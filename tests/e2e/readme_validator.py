"""Validates and executes README curl commands for skill installation."""
import os
import re
import subprocess
import requests

_CURL_URL_RE = re.compile(
    r"curl\s+-sL\s+(https://raw\.githubusercontent\.com/\S+)",
    re.MULTILINE,
)
# Shell variable patterns like $skill, ${SKILL} — not real URLs
_SHELL_VAR_RE = re.compile(r"\$\{?\w+")

_README_PATH = os.path.join(
    os.path.dirname(__file__), "..", "..", "skills", "README.md"
)


class ReadmeValidationError(Exception):
    def __init__(self, url: str, line_number: int, status_code: int):
        self.url = url
        self.line_number = line_number
        self.status_code = status_code
        super().__init__(
            f"README URL returned HTTP {status_code}: {url} (approx. line {line_number})"
        )


class ReadmeValidator:
    def __init__(self, skills_dir: str, readme_path: str | None = None):
        self._skills_dir = skills_dir
        readme_path = readme_path or _README_PATH
        with open(readme_path) as f:
            self._readme_text = f.read()

    def _extract_skill_urls(self) -> list[str]:
        urls = _CURL_URL_RE.findall(self._readme_text)
        # Skip shell variable templates like $skill, ${SKILLS_DIR}
        return [u for u in urls if not _SHELL_VAR_RE.search(u)]

    def _url_line_number(self, url: str) -> int:
        for i, line in enumerate(self._readme_text.splitlines(), 1):
            if url in line:
                return i
        return 0

    def validate_urls(self) -> list[str]:
        """HTTP HEAD each README curl URL. Raises ReadmeValidationError on non-200."""
        urls = self._extract_skill_urls()
        for url in urls:
            resp = requests.head(url, timeout=10, allow_redirects=True)
            if resp.status_code != 200:
                raise ReadmeValidationError(url, self._url_line_number(url), resp.status_code)
        return urls

    def install_skill(self, skill_name: str) -> str:
        """Curl the SKILL.md for skill_name into skills_dir. Returns installed path."""
        urls = self._extract_skill_urls()
        skill_url = next(
            (u for u in urls if f"/{skill_name}/SKILL.md" in u), None
        )
        if not skill_url:
            raise ValueError(f"No curl URL found for skill '{skill_name}' in README")
        dest_dir = os.path.join(self._skills_dir, skill_name)
        os.makedirs(dest_dir, exist_ok=True)
        dest = os.path.join(dest_dir, "SKILL.md")
        subprocess.run(
            ["curl", "-sL", skill_url, "-o", dest],
            check=True,
            timeout=30,
        )
        return dest

    def install_agents_md(self, dest_dir: str) -> str:
        """Curl AGENTS.md from README into dest_dir."""
        urls = self._extract_skill_urls()
        agents_url = next((u for u in urls if u.endswith("AGENTS.md")), None)
        if not agents_url:
            raise ValueError("No AGENTS.md curl URL found in README")
        dest = os.path.join(dest_dir, "AGENTS.md")
        subprocess.run(["curl", "-sL", agents_url, "-o", dest], check=True, timeout=30)
        return dest
