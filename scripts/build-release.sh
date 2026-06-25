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

# Create a clean archive excluding development-only files.
# We use rsync to a temp dir (to handle macOS tar limitations) then tar it.
TMPPARENT=$(mktemp -d)
TMPDIR="${TMPPARENT}/${ARCHIVE_NAME}"
rsync -a \
    --exclude='.git' \
    --exclude='__MACOSX' \
    --exclude='.DS_Store' \
    --exclude='.claude' \
    --exclude='reference' \
    --exclude='target' \
    ./ "${TMPDIR}/"

COPYFILE_DISABLE=1 tar czf "${OUTDIR}/${ARCHIVE_NAME}.tar.gz" -C "${TMPPARENT}" "${ARCHIVE_NAME}"
rm -rf "${TMPPARENT}"

# Generate checksums
cd "${OUTDIR}"
shasum -a 256 "${ARCHIVE_NAME}.tar.gz" > "${ARCHIVE_NAME}.tar.gz.sha256"

echo ""
echo "Release archive: ${OUTDIR}/${ARCHIVE_NAME}.tar.gz"
echo "Checksum:        ${OUTDIR}/${ARCHIVE_NAME}.tar.gz.sha256"
echo ""
cat "${ARCHIVE_NAME}.tar.gz.sha256"
