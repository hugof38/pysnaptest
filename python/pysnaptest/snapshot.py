from __future__ import annotations
from ._pysnaptest import assert_json_snapshot as _assert_json_snapshot
from ._pysnaptest import assert_csv_snapshot as _assert_csv_snapshot
from ._pysnaptest import assert_snapshot as _assert_snapshot
from ._pysnaptest import TestInfo
from typing import Callable, Any, overload, Union, TYPE_CHECKING
from functools import partial, wraps
import asyncio

if TYPE_CHECKING:
    import pandas as pd
    import polars as pl


def extract_from_pytest_env(
    snapshot_path: str | None = None, snapshot_name: str | None = None
) -> TestInfo:
    return TestInfo.from_pytest(
        snapshot_path_override=snapshot_path,
        snapshot_name_override=snapshot_name,
    )


def assert_json_snapshot(
    result: Any, snapshot_path: str | None = None, snapshot_name: str | None = None
):
    test_info = extract_from_pytest_env(snapshot_path, snapshot_name)
    _assert_json_snapshot(test_info, result)


def assert_csv_snapshot(
    result: Any, snapshot_path: str | None = None, snapshot_name: str | None = None
):
    test_info = extract_from_pytest_env(snapshot_path, snapshot_name)
    _assert_csv_snapshot(test_info, result)


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


def assert_dataframe_snapshot(
    df: Union[pd.DataFrame, pl.DataFrame],
    snapshot_path: str | None = None,
    snapshot_name: str | None = None,
    *args,
    **kwargs,
):
    result = None
    if try_is_pandas_df(df):
        result = df.to_csv(*args, **kwargs)

    if try_is_polars_df(df):
        result = df.write_csv(*args, **kwargs)

    if result is None:
        raise ValueError(
            "Unsupported dataframe type, only pandas and polars are supported. "
            "(We may also be unable to import both pandas and polars for some reason, but this is not likely)"
        )
    assert_csv_snapshot(result, snapshot_path, snapshot_name)


def assert_snapshot(
    result: Any, snapshot_path: str | None = None, snapshot_name: str | None = None
):
    test_info = extract_from_pytest_env(snapshot_path, snapshot_name)
    _assert_snapshot(test_info, result)


def insta_snapshot(
    result: Any, snapshot_path: str | None = None, snapshot_name: str | None = None
):
    if isinstance(result, dict) or isinstance(result, list):
        assert_json_snapshot(result, snapshot_path, snapshot_name)
    elif try_is_pandas_df(result) or try_is_polars_df(result):
        assert_dataframe_snapshot(result, snapshot_path, snapshot_name)
    else:
        assert_snapshot(result, snapshot_path, snapshot_name)


@overload
def snapshot(func: Callable) -> Callable: ...


@overload
def snapshot(
    *, filename: str | None = None, folder_path: str | None = None
) -> Callable:  # noqa: F811
    ...


def snapshot(  # noqa: F811
    func: Callable | None = None,
    *,
    snapshot_path: str | None = None,
    snapshot_name: str | None = None,
) -> Callable:
    if asyncio.iscoroutinefunction(func):

        async def asserted_func(func: Callable, *args: Any, **kwargs: Any):
            result = await func(*args, **kwargs)
            insta_snapshot(
                result, snapshot_path=snapshot_path, snapshot_name=snapshot_name
            )

    else:

        def asserted_func(func: Callable, *args: Any, **kwargs: Any):
            result = func(*args, **kwargs)
            insta_snapshot(
                result, snapshot_path=snapshot_path, snapshot_name=snapshot_name
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
