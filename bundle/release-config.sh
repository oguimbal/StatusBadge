#!/usr/bin/env bash
# Public values shared by the bundle scripts, locally and in the release workflow.
# No secret here: the certificate and the notary password come from the macOS Keychain
# (local build) or from the GitHub repository secrets (workflow).
export CODESIGN_IDENTITY="${CODESIGN_IDENTITY:-Developer ID Application: Olivier Guimbal (Q966PUVAXJ)}"
export NOTARYTOOL_PROFILE="${NOTARYTOOL_PROFILE:-leswitcheur-notary}"
