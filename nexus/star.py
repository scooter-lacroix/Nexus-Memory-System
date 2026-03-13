"""Auto-star Nexus Memory System repository."""
import os
import subprocess
from pathlib import Path
from threading import Thread


def star_repo_background():
    """Star the repository in the background if not already attempted."""
    if os.environ.get("NEXUS_NO_STAR") == "1":
        return

    marker = _marker_path()
    if marker.exists():
        return

    Thread(target=_star_repo_impl, daemon=True).start()


def _marker_path() -> Path:
    """Get the path to the star attempt marker file."""
    config_home = os.environ.get("XDG_CONFIG_HOME")
    if config_home:
        config_dir = Path(config_home)
    else:
        config_dir = Path.home() / ".config"
    return config_dir / "nexus-memory-system" / ".star-attempted"


def _star_repo_impl():
    """Attempt to star the repository using gh CLI."""
    marker = _marker_path()
    marker.parent.mkdir(parents=True, exist_ok=True)

    try:
        subprocess.run(
            ["gh", "auth", "status"],
            capture_output=True,
            check=True,
            timeout=2,
        )
    except (subprocess.CalledProcessError, FileNotFoundError, subprocess.TimeoutExpired):
        marker.touch()
        return

    try:
        subprocess.run(
            ["gh", "api", "--silent", "-X", "PUT", "/user/starred/scooter-lacroix/Nexus-Memory-System"],
            capture_output=True,
            timeout=5,
        )
    except (subprocess.CalledProcessError, FileNotFoundError, subprocess.TimeoutExpired):
        pass

    marker.touch()
