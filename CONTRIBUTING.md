# Contributing Guidelines

> **Internal Contribution Guidelines for Nexus Memory System**

**Important:** This is a **private, internal-use project**. External contributions are **not accepted**.

---

## Overview

Nexus Memory System is hosted on GitHub for accessibility only. This is **not an open-source project**.

### Project Status

- **Type:** Private / Internal Use
- **Hosting:** GitHub (accessibility convenience)
- **License:** MIT - Internal Use Only
- **External Contributions:** Not Accepted

---

## For Internal Contributors

This section applies only to authorized internal contributors.

### Development Workflow

1. **Create a branch** from `main`
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Make changes** following coding standards

3. **Test thoroughly**
   ```bash
   pytest
   make lint
   make type-check
   ```

4. **Create internal pull request** for review

### Code Standards

- **Python:** PEP 8 compliant
- **Type hints:** Required for all public functions
- **Docstrings:** Google style docstrings
- **Testing:** Minimum 80% coverage required
- **Linting:** Black and Ruff

### Testing

```bash
# Run all tests
pytest

# Run with coverage
pytest --cov=nexus --cov-report=html

# Run specific tests
pytest tests/unit/test_embeddings.py
```

---

## External Access

### What is Allowed

- **Read-only access** to documentation
- **Read-only access** to code (for reference)
- **Issue creation** for bug reports (will be reviewed internally)

### What is NOT Allowed

- **Pull requests** from external contributors
- **Code contributions** from external sources
- **Forking** for external use
- **Distribution** outside the organization

### Why This Approach?

1. **Quality Control:** Maintain code quality standards
2. **Security:** Prevent security vulnerabilities
3. **Consistency:** Ensure consistent architecture
4. **Support:** Internal support only

---

## Getting Help

### For Internal Users

- **Documentation:** See [docs/](docs/)
- **Issues:** Use internal issue tracker
- **Questions:** Contact the development team directly

### For External Users

**Please note:** We do not provide support for external users. This is an internal system.

- **Documentation:** Publicly available for reference only
- **Issues:** Issues may be created but are not prioritized
- **Questions:** No direct support available

---

## License

```
MIT License

Copyright (c) 2025 scooter-lacroix

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

INTERNAL USE ONLY - External distribution not permitted.
```

---

## Summary

| Aspect | Status |
|--------|--------|
| **Project Type** | Private/Internal |
| **GitHub Access** | Read-only for external |
| **External PRs** | Not Accepted |
| **External Contributions** | Not Accepted |
| **Support** | Internal Only |
| **License** | MIT - Internal Use Only |

---

**Last Updated:** 2025-12-23
