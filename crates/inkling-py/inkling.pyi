"""Type stubs for inkling: reveal ASCII art as a progress indicator."""

from types import TracebackType
from typing import Literal, Optional, Type

__version__: str

Ordering = Literal["auto", "geodesic", "scanline", "reading", "ltr", "rtl"]
ColorDepth = Literal["auto", "truecolor", "24bit", "256", "16", "none", "mono"]
Easing = Literal[
    "linear", "ease-out-cubic", "ease-out-quint", "ease-in-out-cubic"
]
StartHint = Literal["top-left", "bottom", "topological"]

class Loader:
    """A live ASCII-art progress reveal.

    Use it as a context manager so it always finishes cleanly, or call
    :meth:`finish` yourself.

        with Loader(total=len(items), rainbow=True) as bar:
            for item in items:
                work(item)
                bar.inc()
    """

    def __init__(
        self,
        total: Optional[int] = None,
        *,
        art: Optional[str] = None,
        art_path: Optional[str] = None,
        ordering: Optional[Ordering] = None,
        rainbow: bool = False,
        geodesic: bool = False,
        reading: bool = False,
        light: bool = False,
        color: Optional[ColorDepth] = None,
        head: Optional[str] = None,
        body: Optional[str] = None,
        feather: Optional[float] = None,
        easing: Optional[Easing] = None,
        start: Optional[StartHint] = None,
        bridge: Optional[int] = None,
        message: Optional[str] = None,
    ) -> None: ...
    def inc(self, delta: int = 1) -> None:
        """Advance the position by ``delta``."""

    def set(self, pos: int) -> None:
        """Set the absolute position."""

    def set_length(self, total: int) -> None:
        """Change the total amount of work."""

    def set_message(self, message: str) -> None:
        """Set the caption shown beneath the art."""

    def println(self, line: str) -> None:
        """Print a line above the reveal, which redraws beneath it."""

    def finish(self) -> None:
        """Fill the art, leave it on screen, and restore the terminal."""

    def finish_and_clear(self) -> None:
        """Finish and erase the art from the screen."""

    @property
    def position(self) -> int: ...
    @property
    def length(self) -> int:
        """Total units of work, or 0 when indeterminate."""

    @property
    def elapsed(self) -> float:
        """Seconds since the loader started."""

    @property
    def rate(self) -> float:
        """Average units of work per second so far."""

    @property
    def eta(self) -> Optional[float]:
        """Estimated seconds remaining, or None when it cannot be estimated."""

    def __enter__(self) -> "Loader": ...
    def __exit__(
        self,
        exc_type: Optional[Type[BaseException]],
        exc: Optional[BaseException],
        tb: Optional[TracebackType],
    ) -> bool: ...
