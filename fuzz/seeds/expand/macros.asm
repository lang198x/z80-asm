 org 0
m1 macro v, w
 if nul w
 db v
 else
 db w
 endif
 .shift
lbl##v: db v
 endm
 m1 1
 m1 1,2
 rept 3, i, 0, 1
 db i
 endm
 irp x, 1, 2
 db x
 endm
 include "inc.asm"
 m1 5
 if 0
 rept 2
 endif
 endm
 endif
 end
