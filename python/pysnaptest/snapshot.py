from __future__ import annotations
from ._pysnaptest import assert_json_snapshot as _assert_json_snapshot
from ._pysnaptest import assert_csv_snapshot as _assert_csv_snapshot
from ._pysnaptest import assert_snapshot as _assert_snapshot
from ._pysnaptest import assert_binary_snapshot as _assert_binary_snapshot
from ._pysnaptest import TestInfo
from typing import Callable, Any, Dict, overload, Union, Optional, TYPE_CHECKING
from functools import partial, wraps
import asyncio

if TYPE_CHECKING:
    import pandas as pd
    import polars as pl


def extract_from_pytest_env(
    snapshot_path: Optional[str] = None, snapshot_name: Optional[str] = None
) -> TestInfo:
    return TestInfo.from_pytest(
        snapshot_path_override=snapshot_path,
        snapshot_name_override=snapshot_name,
    )


def assert_json_snapshot(
    result: Any,
    snapshot_path: Optional[str] = None,
    snapshot_name: Optional[str] = None,
    redactions: Optional[Dict[str, str]] = None,
):
    test_info = extract_from_pytest_env(snapshot_path, snapshot_name)
    _assert_json_snapshot(test_info, result, redactions)


def assert_csv_snapshot(
    result: Any,
    snapshot_path: Optional[str] = None,
    snapshot_name: Optional[str] = None,
    redactions: Optional[Dict[str, str]] = None,
):
    test_info = extract_from_pytest_env(snapshot_path, snapshot_name)
    _assert_csv_snapshot(test_info, result, redactions)


def try_is_pandas_df(maybe_df: Any) -> bool:
    try:
        import pandas as pd
    except ImportError:
        return False

    return isinstance(maybe_df, pd.DataFrame)


def try_is_polars_df(maybe_df: Any) -> bool:
    try:
        import polars as pl
    except ImportError:
        return False

    return isinstance(maybe_df, pl.DataFrame)


def assert_pandas_dataframe_snapshot(
    df: pd.DataFrame,
    snapshot_path: Optional[str] = None,
    snapshot_name: Optional[str] = None,
    redactions: Optional[Dict[str, str]] = None,
    dataframe_snapshot_format: str = "csv",
    *args,
    **kwargs,
):
    if dataframe_snapshot_format == "csv":
        result = df.to_csv(*args, **kwargs)
        assert_csv_snapshot(result, snapshot_path, snapshot_name, redactions)
    elif dataframe_snapshot_format == "json":
        result = df.to_dict(orient="list", *args, **kwargs)
        assert_json_snapshot(result, snapshot_path, snapshot_name, redactions)
    elif dataframe_snapshot_format == "parquet":
        result = df.to_parquet(engine="pyarrow")
        assert_binary_snapshot(
            result, snapshot_path, snapshot_name, extension=dataframe_snapshot_format
        )
    else:
        raise ValueError(
            "Unsupported snqpshot format for dataframes, supported formats are: 'csv', 'json', 'parquet'."
        )


def assert_polars_dataframe_snapshot(
    df: pl.DataFrame,
    snapshot_path: Optional[str] = None,
    snapshot_name: Optional[str] = None,
    redactions: Optional[Dict[str, str]] = None,
    dataframe_snapshot_format: str = "csv",
    *args,
    **kwargs,
):
    if dataframe_snapshot_format == "csv":
        result = df.write_csv(*args, **kwargs)
        assert_csv_snapshot(result, snapshot_path, snapshot_name, redactions)
    elif dataframe_snapshot_format == "json":
        result = df.to_dict(as_series=False)
        assert_json_snapshot(result, snapshot_path, snapshot_name, redactions)
    elif dataframe_snapshot_format == "bin":
        result = df.serialize(format="binary", *args, **kwargs)
        assert_binary_snapshot(
            result, snapshot_path, snapshot_name, extension=dataframe_snapshot_format
        )
    else:
        raise ValueError(
            "Unsupported snapshot format for polars dataframes, supported formats are: 'csv', 'json', 'bin'."
        )


def assert_dataframe_snapshot(
    df: Union[pd.DataFrame, pl.DataFrame],
    snapshot_path: Optional[str] = None,
    snapshot_name: Optional[str] = None,
    redactions: Optional[Dict[str, str]] = None,
    dataframe_snapshot_format: str = "csv",
    *args,
    **kwargs,
):
    if try_is_pandas_df(df):
        assert_pandas_dataframe_snapshot(
            df,
            snapshot_path,
            snapshot_name,
            redactions,
            dataframe_snapshot_format,
            *args,
            **kwargs,
        )
    elif try_is_polars_df(df):
        assert_polars_dataframe_snapshot(
            df,
            snapshot_path,
            snapshot_name,
            redactions,
            dataframe_snapshot_format,
            *args,
            **kwargs,
        )
    else:
        raise ValueError(
            "Unsupported dataframe type, only pandas and polars are supported. "
            "(We may also be unable to import both pandas and polars for some reason, but this is not likely)"
        )


def assert_binary_snapshot(
    result: bytes,
    snapshot_path: str | None = None,
    snapshot_name: str | None = None,
    extension: str = "bin",
):
    test_info = extract_from_pytest_env(snapshot_path, snapshot_name)
    _assert_binary_snapshot(test_info, extension, result)


def assert_snapshot(
    result: Any, snapshot_path: str | None = None, snapshot_name: str | None = None
):
    test_info = extract_from_pytest_env(snapshot_path, snapshot_name)
    _assert_snapshot(test_info, result)


def insta_snapshot(
    result: Any,
    snapshot_path: Optional[str] = None,
    snapshot_name: Optional[str] = None,
    redactions: Optional[Dict[str, str]] = None,
    dataframe_snapshot_format: str = "csv",
):
    if isinstance(result, dict) or isinstance(result, list):
        assert_json_snapshot(result, snapshot_path, snapshot_name, redactions)
    elif isinstance(result, bytes):
        assert_binary_snapshot(
            result, snapshot_path, snapshot_name, extension=dataframe_snapshot_format
        )
    elif try_is_pandas_df(result) or try_is_polars_df(result):
        assert_dataframe_snapshot(
            result, snapshot_path, snapshot_name, redactions, dataframe_snapshot_format
        )
    else:
        if redactions is not None:
            raise ValueError("Redactions may only be used with json or csv snapshots.")
        assert_snapshot(result, snapshot_path, snapshot_name)


@overload
def snapshot(func: Callable) -> Callable: ...


@overload
def snapshot(
    *,
    filename: Optional[str] = None,
    folder_path: Optional[str] = None,
    redactions: Optional[Dict[str, str]] = None,
    dataframe_snapshot_format: str = "csv",
) -> Callable:  # noqa: F811
    ...


def snapshot(  # noqa: F811
    func: Optional[Callable] = None,
    *,
    snapshot_path: Optional[str] = None,
    snapshot_name: Optional[str] = None,
    redactions: Optional[Dict[str, str]] = None,
    dataframe_snapshot_format: str = "csv",
) -> Callable:
    if asyncio.iscoroutinefunction(func):

        async def asserted_func(func: Callable, *args: Any, **kwargs: Any):
            result = await func(*args, **kwargs)
            insta_snapshot(
                result,
                snapshot_path=snapshot_path,
                snapshot_name=snapshot_name,
                redactions=redactions,
                dataframe_snapshot_format=dataframe_snapshot_format,
            )

    else:

        def asserted_func(func: Callable, *args: Any, **kwargs: Any):
            result = func(*args, **kwargs)
            insta_snapshot(
                result,
                snapshot_path=snapshot_path,
                snapshot_name=snapshot_name,
                redactions=redactions,
                dataframe_snapshot_format=dataframe_snapshot_format,
            )

    # Without arguments `func` is passed directly to the decorator
    if func is not None:
        if not callable(func):
            raise TypeError("Not a callable. Did you use a non-keyword argument?")
        return wraps(func)(partial(asserted_func, func))

    # With arguments, we need to return a function that accepts the function
    def decorator(func: Callable) -> Callable:
        return wraps(func)(partial(asserted_func, func))

    return decorator
