border  macro colour
 ld a,colour
 endm
start: ld (ix-1+3),HIGH $+1
 border 1,,(2
 include it's.asm
 .error a ; b
 db 1 ? 2 : DEFINED foo
ld a,(2)*3
