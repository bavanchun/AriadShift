---
title: Cloud worker boundary verification
date: 2026-10-25
summary: Completed the procfs open-only probe and prepared final validation of the upload and worker boundaries.
---

# Cloud worker boundary verification

The upload sweep now requires a successful post-expiry check, while real-service controls preserve valid completed inputs. Worker upload grants are read-only, and helper-filter startup preflight rejects refused sandbox syscalls in every mode.

The remaining procfs test was repaired with the approved edit: opening memory replaces reading offset zero, which can fail even when access is allowed. The previous local and hosted server suites passed. Final Linux verification and pull request readiness are tracked in PR #47. The direct-launch Kubernetes fragment remains explicitly documented as untested.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
