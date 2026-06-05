//! Compressed snapshot support.
//!
//! Compressed snapshots are stored on disk as binary insta snapshots (the
//! compressed bytes), but comparison happens on the *decompressed* contents.
//! This means that two snapshots whose compressed representations differ (for
//! example because gzip embeds a timestamp in its header) still match as long
//! as the data they encode is identical.
//!
//! The comparison logic lives in [`CompressionComparator`], which implements
//! insta's [`Comparator`] trait. When the decompressed contents differ it
//! prints a readable unified diff of the decompressed strings, since insta only
//! shows binary file links for binary snapshots otherwise.

use std::borrow::Cow;
use std::io::Read;

use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use insta::comparator::Comparator;
use insta::internals::SnapshotContents;
use insta::Snapshot;
use pyo3::exceptions::PyValueError;
use pyo3::PyResult;
use similar::{Algorithm, TextDiff};

/// Compression algorithms supported by [`assert_compressed_snapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionAlgorithm {
    Gzip,
    Zlib,
    Deflate,
}

impl CompressionAlgorithm {
    /// Parse an algorithm name, raising a Python `ValueError` for unknown values.
    pub fn parse(algorithm: &str) -> PyResult<Self> {
        match algorithm {
            "gzip" | "gz" => Ok(Self::Gzip),
            "zlib" => Ok(Self::Zlib),
            "deflate" => Ok(Self::Deflate),
            other => Err(PyValueError::new_err(format!(
                "Unsupported compression algorithm: '{other}'. Supported algorithms are: 'gzip', 'zlib', 'deflate'."
            ))),
        }
    }

    /// File extension used when storing the binary snapshot on disk.
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Gzip => "gz",
            Self::Zlib => "zz",
            Self::Deflate => "deflate",
        }
    }

    /// Decompress `data` according to this algorithm.
    pub fn decompress(&self, data: &[u8]) -> std::io::Result<Vec<u8>> {
        let mut reader: Box<dyn Read> = match self {
            Self::Gzip => Box::new(GzDecoder::new(data)),
            Self::Zlib => Box::new(ZlibDecoder::new(data)),
            Self::Deflate => Box::new(DeflateDecoder::new(data)),
        };
        let mut out = Vec::new();
        reader.read_to_end(&mut out)?;
        Ok(out)
    }
}

/// Borrow the raw bytes backing a snapshot, regardless of whether it is stored
/// as a binary or text snapshot. The common (binary) case borrows; only text
/// snapshots need an owned copy.
fn snapshot_bytes(snapshot: &Snapshot) -> Cow<'_, [u8]> {
    match snapshot.contents() {
        SnapshotContents::Binary(items) => Cow::Borrowed(items.as_slice()),
        SnapshotContents::Text(text) => Cow::Owned(text.to_string().into_bytes()),
    }
}

/// Print a unified diff of two decompressed payloads to stderr, using the same
/// diff method insta uses for text snapshots (the Patience line diff).
fn print_decompressed_diff(reference: &[u8], test: &[u8]) {
    let reference = String::from_utf8_lossy(reference);
    let test = String::from_utf8_lossy(test);
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .diff_lines(reference.as_ref(), test.as_ref());
    eprintln!("\nCompressed snapshot mismatch (showing decompressed unified diff):");
    eprint!(
        "{}",
        diff.unified_diff()
            .header("stored (decompressed)", "new (decompressed)")
    );
    eprintln!();
}

/// An insta [`Comparator`] that compares binary snapshots on their decompressed
/// contents rather than their raw (compressed) bytes.
#[derive(Debug, Clone)]
pub struct CompressionComparator {
    algorithm: CompressionAlgorithm,
}

impl CompressionComparator {
    pub fn new(algorithm: CompressionAlgorithm) -> Self {
        Self { algorithm }
    }
}

impl Comparator for CompressionComparator {
    fn matches(&self, reference: &Snapshot, test: &Snapshot) -> bool {
        let reference_bytes = snapshot_bytes(reference);
        let test_bytes = snapshot_bytes(test);

        match (
            self.algorithm.decompress(reference_bytes.as_ref()),
            self.algorithm.decompress(test_bytes.as_ref()),
        ) {
            (Ok(reference_decompressed), Ok(test_decompressed)) => {
                if reference_decompressed == test_decompressed {
                    true
                } else {
                    print_decompressed_diff(&reference_decompressed, &test_decompressed);
                    false
                }
            }
            // If either side fails to decompress, fall back to comparing the
            // raw bytes so the mismatch is still surfaced.
            _ => reference_bytes == test_bytes,
        }
    }

    fn dyn_clone(&self) -> Box<dyn Comparator> {
        Box::new(self.clone())
    }
}
