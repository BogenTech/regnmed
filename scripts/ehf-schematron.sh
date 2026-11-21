#!/bin/sh
# Validates rendered EHF against the official Peppol BIS Billing 3.0 rules
# (docs/efaktura.md §3.4): EN 16931, the Peppol rules and the Norwegian ones.
# XSD only proves well-formed UBL; these are the rules an access point
# rejects on.
#
# The rule files are fetched, not vendored: OpenPEPPOL's repository states no
# licence we could redistribute them under in this public repository. Every
# download is pinned by SHA-256, so a new rule version is a commit, and it
# belongs on the regelverksrevisjon's checklist (Peppol releases twice a year).
#
# CI only, never in production. Needs Java (JAVA_CMD can name a container).
set -eu
DIR=${1:-target/ehf-schematron}
JAVA_CMD=${JAVA_CMD:-java}
RULES=https://raw.githubusercontent.com/OpenPEPPOL/peppol-bis-invoice-3/38d693d71d28d813781323a6ca97324edcc4d239/rules/sch
MAVEN=https://repo1.maven.org/maven2
mkdir -p "$DIR"

fetch() { # file url sha256
    if [ -f "$DIR/$1" ] && echo "$3  $DIR/$1" | shasum -a 256 -c - >/dev/null 2>&1; then
        return
    fi
    curl -sSfL -o "$DIR/$1.part" "$2"
    echo "$3  $DIR/$1.part" | shasum -a 256 -c - >/dev/null || {
        echo "FAIL  $1: checksum mismatch -- the published file changed" >&2
        rm -f "$DIR/$1.part"
        exit 1
    }
    mv "$DIR/$1.part" "$DIR/$1"
}
fetch CEN-EN16931-UBL.sch "$RULES/CEN-EN16931-UBL.sch" 268d4f7a2688676695e6c69cba6fba69a6802604fee12cb544a6b30ff09555a3
fetch PEPPOL-EN16931-UBL.sch "$RULES/PEPPOL-EN16931-UBL.sch" f637444fab75a6da7e5004f94a6de8e71383ef5658dafe6c68925103b17ea148
fetch saxon.jar "$MAVEN/net/sf/saxon/Saxon-HE/10.9/Saxon-HE-10.9.jar" 491d8edf4ec811d15c2b2417b007218b9b938f15e4dfbad004025beb4e70e960
fetch schxslt.jar "$MAVEN/name/dmaus/schxslt/schxslt/1.10.1/schxslt-1.10.1.jar" 4f4f21edab7b37f96ad59ae12a344d3510f1092ac46b6d81a4efa0120b73cb58

saxon() { $JAVA_CMD -cp "$DIR/saxon.jar" net.sf.saxon.Transform "$@"; }

# Saxon's command line only opens a stylesheet that exists as a file.
unzip -qo "$DIR/schxslt.jar" 'xslt/*' -d "$DIR/schxslt"
PIPELINE="$DIR/schxslt/xslt/2.0/pipeline-for-svrl.xsl"
for rules in CEN-EN16931-UBL PEPPOL-EN16931-UBL; do
    if [ ! -f "$DIR/$rules.xsl" ] || [ "$DIR/$rules.sch" -nt "$DIR/$rules.xsl" ]; then
        saxon -s:"$DIR/$rules.sch" -xsl:"$PIPELINE" -o:"$DIR/$rules.xsl"
    fi
done

rm -rf "$DIR/matrix" "$DIR/reports"
mkdir -p "$DIR/matrix" "$DIR/reports"
EHF_MATRIX_DIR="$PWD/$DIR/matrix" cargo test -q -p regnmed-core --lib \
    ehf::tests::writes_the_schematron_matrix >/dev/null

count=0
failed=0
for xml in "$DIR"/matrix/*.xml; do
    name=$(basename "$xml" .xml)
    count=$((count + 1))
    for rules in CEN-EN16931-UBL PEPPOL-EN16931-UBL; do
        report="$DIR/reports/$name-$rules.svrl"
        saxon -s:"$xml" -xsl:"$DIR/$rules.xsl" -o:"$report"
        # NO-R-002 is only a warning in Peppol, but the statement is required by law.
        must="//*[local-name()='failed-assert'][@flag='fatal' or @id='NO-R-002']"
        fatal=$(xmllint --xpath "count($must)" "$report")
        if [ "$fatal" != 0 ]; then
            failed=$((failed + fatal))
            echo "FAIL  $name ($rules)" >&2
            xmllint --xpath "$must" "$report" |
                sed -E 's/></>\n</g' | grep -E 'failed-assert |<svrl:text>' | cut -c1-260 >&2
        fi
    done
done
[ "$count" -gt 0 ] || { echo "FAIL  the matrix is empty" >&2; exit 1; }
[ "$failed" -eq 0 ] || { echo "$failed Schematron rule failure(s)" >&2; exit 1; }
echo "ok    $count documents pass the Peppol BIS 3.0 rules"
