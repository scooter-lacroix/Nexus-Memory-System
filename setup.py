"""
Setup script for Nexus Memory System
"""

from setuptools import setup, find_packages
import os

# Read README file
with open("README.md", "r", encoding="utf-8") as fh:
    long_description = fh.read()

# Read requirements
with open("requirements.txt", "r", encoding="utf-8") as fh:
    requirements = [line.strip() for line in fh if line.strip() and not line.startswith("#")]

setup(
    name="nexus-memory-system",
    use_scm_version={"write_to": "nexus/_version.py"},
    setup_requires=['setuptools_scm'],
    author="scooter-lacroix",
    author_email="scooter.lacroix@example.com",
    description="A comprehensive, cross-agent memory management platform",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/scooter-lacroix/nexus-memory-system",
    packages=find_packages(),
    classifiers=[
        "Development Status :: 5 - Production/Stable",
        "Intended Audience :: Developers",
        "License :: OSI Approved :: MIT License",
        "Operating System :: OS Independent",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Programming Language :: Python :: 3.12",
        "Programming Language :: Python :: 3.13",
        "Topic :: Scientific/Engineering :: Artificial Intelligence",
        "Topic :: Software Development :: Libraries :: Python Modules",
    ],
    python_requires=">=3.9",
    install_requires=requirements,
    extras_require={
        "dev": [
            "pytest>=8.3.0",
            "pytest-asyncio>=0.24.0",
            "pytest-cov>=6.0.0",
            "black>=24.10.0",
            "ruff>=0.8.0",
            "mypy>=1.14.0",
            "pre-commit>=4.0.0",
        ],
        "postgres": [
            "asyncpg>=0.30.0",
            "psycopg2-binary>=2.9.0",
        ],
        "embeddings": [
            "sentence-transformers>=3.3.0",
            "torch>=2.5.0",
        ],
    },
    entry_points={
        "console_scripts": [
            "nexus=nexus.cli:main",
            "nexus-serve=nexus.main:run",
            "nexus-ui=nexus.web_ui.app:run_web_ui",
        ],
    },
    include_package_data=True,
    package_data={
        "nexus": [
            "web_ui/static/**/*",
            "web_ui/templates/**/*",
            "agents/scripts/**/*",
            "agents/configs/**/*",
        ],
    },
)