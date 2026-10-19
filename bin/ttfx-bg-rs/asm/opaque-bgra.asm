; Stateless raster kernel. No engine state or callbacks.
; SysV AMD64: dst=RDI src=RSI width_pixels=RDX height=RCX
;             dst_stride_bytes=R8 src_stride_bytes=R9.
; Preconditions: independent readable/writable rectangles, checked spans,
; premultiplied BGRA source. Retained opaque/backgroundNone semantics only.
; Preserve BGR exactly and set alpha255. No allocation, syscalls or callbacks.
bits 64
cpu x86-64
default rel

section .text
global ttfx_bgra_copy_opaque
ttfx_bgra_copy_opaque:
    test    rdx, rdx
    jz      .done
    test    rcx, rcx
    jz      .done
    movdqa  xmm1, [alpha_mask]
.row:
    mov     r10, rdx
    xor     r11d, r11d
    cmp     r10, 4
    jb      .tail
.vector:
    ; Four complete pixels only: never read past a row or into its padding.
    movdqu  xmm0, [rsi + r11]
    por     xmm0, xmm1
    movdqu  [rdi + r11], xmm0
    add     r11, 16
    sub     r10, 4
    cmp     r10, 4
    jae     .vector
.tail:
    test    r10, r10
    jz      .next_row
.pixel:
    mov     eax, [rsi + r11]
    or      eax, 0xff000000
    mov     [rdi + r11], eax
    add     r11, 4
    dec     r10
    jnz     .pixel
.next_row:
    ; Base pointers remain at each row start; stride includes painted bytes.
    add     rsi, r9
    add     rdi, r8
    dec     rcx
    jnz     .row
.done:
    ret

section .rodata align=16
align 16
alpha_mask: times 4 dd 0xff000000

section .note.GNU-stack noalloc noexec nowrite progbits
