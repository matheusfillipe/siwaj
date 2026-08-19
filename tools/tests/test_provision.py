from pathlib import Path

from siwaj_tools.provision import load_env


def write_env(tmp_path: Path, content: str) -> Path:
    env = tmp_path / ".env"
    env.write_text(content, encoding="utf-8")
    return env


def test_load_env_skips_comments_blanks_and_empty(tmp_path: Path) -> None:
    env = write_env(
        tmp_path,
        "# comment\n\nOPENWEATHER_API_KEY=abc123\nEMPTY=\nSPACED = spaced \n",
    )
    assert load_env(env) == {"OPENWEATHER_API_KEY": "abc123", "SPACED": "spaced"}


def test_load_env_empty_file(tmp_path: Path) -> None:
    env = write_env(tmp_path, "")
    assert load_env(env) == {}
