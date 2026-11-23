#!/bin/sh
# Verify the linked Linux desktop, then read and transform a mesh-bound parameter series.
set -eu
export LC_ALL=C
[ "$#" -eq 2 ] || { echo "Usage: $0 fieldcraft fieldcraft-cli" >&2; exit 2; }
gui=$(realpath "$1")
cli=$(realpath "$2")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# Run the installed binaries in an isolated profile, even on an interactive build host.
export HOME="$work/home" XDG_DATA_HOME="$work/data" FIELDCRAFT_RECOVERY_DIR="$work/data/recovery"
mkdir -p "$HOME" "$FIELDCRAFT_RECOVERY_DIR"
cd "$work"
for binary in "$gui" "$cli"; do
    readelf -h "$binary" > header
    grep -Eq 'Class: +ELF64$' header && grep -Eq 'Machine: +Advanced Micro Devices X86-64$' header || {
        echo "Expected Linux amd64 ELF64 executable: $binary" >&2; exit 1;
    }
done
[ "$("$gui" --version)" = "fieldcraft 0.8.1-ngsolve.r1" ]
[ "$("$cli" --version)" = "fieldcraft-cli 0.8.1-ngsolve.r1" ]
"$cli" commands --json > commands.json
perl -MJSON::PP -0777 -ne '
    my %c = map { $_->{id} => $_ } @{decode_json($_)};
    exit !($c{"results.external.deformed"} && $c{"results.external.playback"}
           && $c{"results.external.series"}{schema}{properties}{deformation_scale}
           && $c{"results.external.contour"}{schema}{properties}{show}{default})
' commands.json
cat > mesh.json <<'JSON'
[{"command":"mesh.node.add","params":{"label":7,"xyz":[0,0,0]}},
 {"command":"mesh.node.add","params":{"label":9,"xyz":[1,0,0]}},
 {"command":"mesh.element.add","params":{"label":1,"kind":"rod2","nodes":[7,9]}},
 {"command":"file.save_as","params":{"path":"model.fieldcraft"}}]
JSON
"$cli" run mesh.json > mesh.log
unzip -p model.fieldcraft mesh.bin > mesh.bin
test -s mesh.bin
mesh=$(sha256sum mesh.bin | cut -d' ' -f1)
for index in 0 1; do
    printf '{"format":"fieldcraft-nodal-field/1","mesh_sha256":"%s","name":"U","unit":"m","components":3,"node_ids":[7,9],"values":[0,0,0,0,%s,0]}\n' "$mesh" "$index" > "u$index.json"
done
printf '{"format":"fieldcraft-nodal-series/2","axis":{"label":"Voltage","unit":"V"},"mesh_sha256":"%s","name":"U","unit":"m","components":3,"frames":[{"file":"u0.json","parameter":-4},{"file":"u1.json","parameter":16}]}\n' "$mesh" > sweep.json
perl - "$cli" <<'PERL'
use strict;
use warnings;
use IPC::Open2;
use JSON::PP;
my $json = JSON::PP->new->canonical->allow_nonref;
my $pid = open2(my $from_cli, my $to_cli, $ARGV[0], "mcp", "--headless");
my $id = 0;
sub request {
    my ($method, $params) = @_;
    ++$id;
    print {$to_cli} $json->encode({jsonrpc => "2.0", id => $id, method => $method, params => $params}), "\n";
    my $line = <$from_cli> // die "MCP closed while running $method\n";
    my $reply = $json->decode($line);
    die "Unexpected MCP response for $method: $line" unless ($reply->{id} // -1) == $id && exists $reply->{result};
    return $reply->{result};
}
sub check_command {
    my ($command, $params, $expected) = @_;
    my $result = request("tools/call", {name => "execute", arguments => {command => $command, params => $params}});
    my $text = $result->{content}[0]{text} // die "Missing reply for $command\n";
    die "$command failed: $text\n" if $result->{isError};
    my $values = $json->decode($text);
    for my $key (sort keys %$expected) {
        die "Unexpected $command $key: $text\n" unless exists $values->{$key}
          && $json->encode($values->{$key}) eq $json->encode($expected->{$key});
    }
    print "$command ", $json->encode($values), "\n" if keys %$expected;
}
request("initialize", {protocolVersion => "2025-06-18", capabilities => {}, clientInfo => {name => "r2-smoke", version => "1"}});
print {$to_cli} qq({"jsonrpc":"2.0","method":"notifications/initialized"}\n);
my $axis = {label => "Voltage", unit => "V"};
check_command("file.open", {path => "model.fieldcraft"}, {});
check_command("results.external.series", {path => "sweep.json", component => "y", deformation_scale => 2},
              {field => "U", frames => 2, axis => $axis, parameter => -4, range => [0, 1], shown => JSON::PP::true});
check_command("results.external.playback", {frame => 1}, {frame => 1, frames => 2, axis => $axis, parameter => 16});
check_command("results.external.deformed", {scale => 4}, {deformed => JSON::PP::true, scale => 4});
check_command("results.external.contour", {show => JSON::PP::false}, {shown => JSON::PP::false});
close($to_cli) or die "Cannot close MCP input: $!\n";
waitpid($pid, 0) == $pid && $? == 0 or die "MCP exited unsuccessfully\n";
PERL
printf '%s\n' "Verified mesh $mesh and all parameter-series responses."
