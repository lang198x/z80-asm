"""Measures which instruction lines PasmoNext 0.1.3 warns "looks like a non
existent instruction" about.

Each line PasmoNext accepted, as listed in
crates/z80-asm/tests/data/pasmonext-0.1.3-instructions.tsv, is assembled
again, one line per run at org 0, and the lines it gives the warning are
written out. It gives it only in its second pass, so once a line.

Usage: SWEEP_TMP=<scratch dir> python3 measure-warnings.py <out.tsv>
Needs PasmoNext 0.1.3 on the PATH as `pasmo`. The output is sorted and given
a header to become crates/z80-asm/tests/data/pasmonext-0.1.3-looks.tsv.
"""
import os, subprocess, sys, tempfile
from multiprocessing import Pool

ACCEPTED = os.path.join(os.path.dirname(__file__),
    "../crates/z80-asm/tests/data/pasmonext-0.1.3-instructions.tsv")
WARNING = "looks like a non existent instruction"

def run(line):
    d = tempfile.mkdtemp(dir=os.environ["SWEEP_TMP"])
    src, out = os.path.join(d, "t.asm"), os.path.join(d, "t.bin")
    with open(src, "w") as f:
        f.write(f" org 0\n {line}\n")
    r = subprocess.run(["pasmo", src, out], capture_output=True, text=True,
        timeout=10)
    warned = WARNING in r.stdout + r.stderr
    for p in (src, out):
        if os.path.exists(p):
            os.remove(p)
    os.rmdir(d)
    return line, warned

if __name__ == "__main__":
    lines = [l.split("\t")[0] for l in open(ACCEPTED)
        if not l.startswith("#") and "\t" in l]
    with Pool(10) as pool, open(sys.argv[1], "w") as out:
        warned = 0
        for line, warns in pool.imap(run, lines, chunksize=32):
            if warns:
                warned += 1
                out.write(f"{line}\n")
        print(f"{len(lines)} lines, {warned} warned")
