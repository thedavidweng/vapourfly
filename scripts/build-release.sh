#!/usr/bin/env bash
# Build a clean Vapourfly release archive.
#
# Usage: ./scripts/build-release.sh [version]
#
# Produces:
#   target/release-artifacts/vapourfly-{version}-source.tar.gz
#   target/release-artifacts/vapourfly-{version}-source.tar.gz.sha256
#
# The archive excludes:
#   .git/, __MACOSX/, .DS_Store, .claude/, reference/, target/

set -euo pipefail

VERSION="${1:-$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')}"
OUTDIR="target/release-artifacts"
ARCHIVE_NAME="vapourfly-${VERSION}-source"

echo "Building release archive for version: ${VERSION}"

# Clean output directory
rm -rf "${OUTDIR}"
mkdir -p "${OUTDIR}"

# Create a clean export using git archive if in a git repo, otherwise use tar
if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    # Use git archive for a clean export
    git archive \
        --format=tar.gz \
        --prefix="${ARCHIVE_NAME}/" \
        --output="${OUTDIR}/${ARCHIVE_NAME}.tar.gz" \
        HEAD
else
    # Fallback: create tar excluding unwanted files
    tar czf "${OUTDIR}/${ARCHIVE_NAME}.tar.gz" \
        --exclude='.git' \
        --exclude='__MACOSX' \
        --exclude='.DS_Store' \
        --exclude='.claude' \
        --exclude='reference' \
        --exclude='target' \
        --exclude='*.DS_Store' \
        --transform="s,^.,${ARCHIVE_NAME}," \
        .
fi

# Generate checksums
cd "${OUTDIR}"
shasum -a 256 "${ARCHIVE_NAME}.tar.gz" > "${ARCHIVE_NAME}.tar.gz.sha256"

echo ""
echo "Release archive: ${OUTDIR}/${ARCHIVE_NAME}.tar.gz"
echo "Checksum:        ${OUTDIR}/${ARCHIVE_NAME}.tar.gz.sha256"
echo ""
cat "${ARCHIVE_NAME}.tar.gz.sha256"
