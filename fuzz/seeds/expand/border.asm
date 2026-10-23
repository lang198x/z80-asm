; Cycle the border
        org $8000
start:  ld a,%111
        ex af,af'
        db "a\n",'it''s',0FFh,&HFF,#1$2 // c
