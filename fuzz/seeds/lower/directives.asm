 org 8000h
start: ld a,(ix+a)
 db "hi",0
 dw start
 ds 2, b
 align 4
 incbin data.bin
x equ a+1
 end start
