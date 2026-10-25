"""Measures which instruction lines PasmoNext 0.1.3 warns about under --w8080.

Each line PasmoNext accepted, as listed in
crates/z80-asm/tests/data/pasmonext-0.1.3-instructions.tsv, is assembled
again with --w8080, one line per run at org 0, and the lines it gives no
"not a 8080 instruction" warning are written out.

Usage: SWEEP_TMP=<scratch dir> python3 measure-8080-warnings.py <out.tsv>
Needs PasmoNext 0.1.3 on the PATH as `pasmo`. The output is sorted and given
a header to become crates/z80-asm/tests/data/pasmonext-0.1.3-8080.tsv.
"""
import os, subprocess, sys, tempfile
from multiprocessing import Pool

ACCEPTED = os.path.join(os.path.dirname(__file__),
    "../crates/z80-asm/tests/data/pasmonext-0.1.3-instructions.tsv")

def run(line):
    d = tempfile.mkdtemp(dir=os.environ["SWEEP_TMP"])
    src, out = os.path.join(d, "t.asm"), os.path.join(d, "t.bin")
    with open(src, "w") as f:
        f.write(f" org 0\n {line}\n")
    r = subprocess.run(["pasmo", "--w8080", src, out], capture_output=True,
        text=True, timeout=10)
    warned = "not a 8080 instruction" in r.stdout + r.stderr
    for p in (src, out):
        if os.path.exists(p):
            os.remove(p)
    os.rmdir(d)
    return line, warned

if __name__ == "__main__":
    lines = [l.split("\t")[0] for l in open(ACCEPTED)
        if not l.startswith("#") and "\t" in l]
    with Pool(10) as pool, open(sys.argv[1], "w") as out:
        quiet = 0
        for line, warned in pool.imap(run, lines, chunksize=32):
            if not warned:
                quiet += 1
                out.write(f"{line}\n")
        print(f"{len(lines)} lines, {quiet} with no warning")
