    .section .text
    .globl __switch
__switch:
    sd  s0,  0*8(a0)
    sd  s1,  1*8(a0)
    sd  s2,  2*8(a0)
    sd  s3,  3*8(a0)
    sd  s4,  4*8(a0)
    sd  s5,  5*8(a0)
    sd  s6,  6*8(a0)
    sd  s7,  7*8(a0)
    sd  s8,  8*8(a0)
    sd  s9,  9*8(a0)
    sd s10, 10*8(a0)
    sd s11, 11*8(a0)
    sd  ra, 12*8(a0)
    sd  sp, 13*8(a0)

    ld  s0,  0*8(a1)
    ld  s1,  1*8(a1)
    ld  s2,  2*8(a1)
    ld  s3,  3*8(a1)
    ld  s4,  4*8(a1)
    ld  s5,  5*8(a1)
    ld  s6,  6*8(a1)
    ld  s7,  7*8(a1)
    ld  s8,  8*8(a1)
    ld  s9,  9*8(a1)
    ld s10, 10*8(a1)
    ld s11, 11*8(a1)
    ld  ra, 12*8(a1)
    ld  sp, 13*8(a1)
    ret
    