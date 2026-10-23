"""Measures which instruction lines PasmoNext 0.1.3 accepts, and their bytes.

Every mnemonic in the profile is assembled with 0, 1 or 2 operands from the
vocabulary below, one line per run at org 0, and the accepted lines are
written with the bytes PasmoNext wrote. The vocabulary and mnemonics must
match the sweep test in crates/z80-asm/src/lower/instruction/tests.rs, which
treats every unlisted line as rejected.

Usage: SWEEP_TMP=<scratch dir> python3 measure-instructions.py <out.tsv>
Needs PasmoNext 0.1.3 on the PATH as `pasmo`. The output is sorted and given
a header to become crates/z80-asm/tests/data/pasmonext-0.1.3-instructions.tsv.
"""
import os, subprocess, sys, tempfile, itertools
from multiprocessing import Pool

MNEMONICS = """adc add and bit call ccf cp cpd cpdr cpi cpir cpl daa dec di djnz ei ex exx halt im in inc ind indr ini inir jp jr ld ldd lddr ldi ldir neg nop or otdr otir out outd outi pop push res ret reti retn rl rla rlc rlca rld rr rra rrc rrca rrd rst sbc scf set sla sll sra srl sub xor brlc bsla bsra bsrf bsrl ldirx ldix lddrx lddx ldpirx ldws mirror mul nextreg outinb pixelad pixeldn setae swapnib test ldirscale popx fillde""".split()
VOCAB = ["a","b","c","d","e","h","l","i","r","ixh","ixl","iyh","iyl",
    "af","af'","bc","de","hl","sp","ix","iy",
    "nz","z","nc","po","pe","p","m","f",
    "(hl)","(bc)","(de)","(sp)","(c)","(ix)","(iy)","(ix+5)","(iy+5)","(ix-5)",
    "[hl]","[c]","[ix+5]","[1234h]","(12h)","(1234h)",
    "0","1","2","7","8","10h","38h","12h","1234h","$+7"]

def lines():
    for m in MNEMONICS:
        yield m
        for a in VOCAB:
            yield f"{m} {a}"
        for a, b in itertools.product(VOCAB, VOCAB):
            yield f"{m} {a},{b}"

def run(line):
    d = tempfile.mkdtemp(dir=os.environ["SWEEP_TMP"])
    src, out = os.path.join(d, "t.asm"), os.path.join(d, "t.bin")
    with open(src, "w") as f:
        f.write(f" org 0\n {line}\n")
    subprocess.run(["pasmo", src, out], capture_output=True, timeout=10)
    result = open(out, "rb").read().hex() if os.path.exists(out) else None
    for p in (src, out):
        if os.path.exists(p):
            os.remove(p)
    os.rmdir(d)
    return line, result

if __name__ == "__main__":
    with Pool(10) as pool, open(sys.argv[1], "w") as out:
        n = accepted = 0
        for line, result in pool.imap(run, lines(), chunksize=64):
            n += 1
            if result is not None:
                accepted += 1
                out.write(f"{line}\t{result}\n")
        print(f"{n} lines, {accepted} accepted")
