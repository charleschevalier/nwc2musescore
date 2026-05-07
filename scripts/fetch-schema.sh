#!/usr/bin/env bash
# Fetch the MusicXML 4.0 XSD schema files from the W3C MusicXML repo and
# patch the absolute http:// imports to relative paths so xmllint can use
# them with --nonet.
#
# Run from the repository root:
#   ./scripts/fetch-schema.sh
set -euo pipefail

DEST="${1:-schema/musicxml-4.0}"
mkdir -p "$DEST"

base="https://raw.githubusercontent.com/w3c/musicxml/v4.0/schema"
for f in musicxml.xsd xml.xsd xlink.xsd; do
    echo "fetching $f ..."
    curl -fsSL "$base/$f" -o "$DEST/$f"
done

# Rewrite the absolute http:// imports in musicxml.xsd to local file refs.
sed -i \
    -e 's|http://www.musicxml.org/xsd/xml.xsd|xml.xsd|' \
    -e 's|http://www.musicxml.org/xsd/xlink.xsd|xlink.xsd|' \
    "$DEST/musicxml.xsd"

echo "done. schema in $DEST"
echo "validate with:  xmllint --noout --nonet --schema $DEST/musicxml.xsd <file.musicxml>"
