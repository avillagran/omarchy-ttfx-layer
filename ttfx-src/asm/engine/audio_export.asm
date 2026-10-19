; Current-frame audio for typed direct output. Scene and visual caches keep
; authored colors. The synchronous consumer borrows a reusable export pool;
; source visuals, scene handles and renderer state are never modified.
; Inactive audio and --no-color borrow the original grid/pool byte-for-byte.
; Scalar SSE2 audio_tint is shared with the existing engine (all x86 tiers).

section .text

; visual_export_audio -> rcx=borrowed grid, r8=borrowed pool.
; Preserves SysV callee-saved registers. No C calls and no per-frame mmap.
visual_export_audio:
    mov     qword [aexp_active], 0
    mov     qword [aexp_unique], 0
    mov     qword [aexp_changed], 0
    mov     rcx, [handle_grid]
    mov     r8, [pool_base]
    cmp     byte [audio_color_on], 0
    je      .raw
    cmp     byte [cfg_no_color], 0
    jne     .raw
    mov     qword [aexp_active], 1
    push    rbx
    push    rbp
    push    r12
    push    r13
    push    r14
    push    r15
    cmp     qword [aexp_pool], 0
    jne     .ready
    mov     rdi, [grid_cells]
    shl     rdi, 2
    add     rdi, 64                    ; borrowed grid's readable slack
    call    alloc
    mov     [aexp_grid], rax
    ; At most one mapping per occupied grid cell. Load <= 1/2 guarantees
    ; bounded probing, including the reserved blank, without resize.
    mov     rcx, [grid_cells]
    inc     rcx
    add     rcx, rcx
    mov     edi, 1
.capacity:
    cmp     rdi, rcx
    jae     .allocate_map
    shl     rdi, 1
    jmp     .capacity
.allocate_map:
    lea     rax, [rdi - 1]
    mov     [aexp_map_mask], rax
    shl     rdi, 4                    ; epoch u64, source u32, export u32
    call    alloc
    mov     [aexp_map], rax
    mov     rdi, POOL_RESERVE
    call    reserve
    mov     [aexp_pool], rax
.ready:
    inc     qword [aexp_epoch]
    ; Preserve the canonical blank at offset32 for bridge fast-skip. Its
    ; absent fg/bg remains absent: audio cannot create a visible empty cell.
    mov     rsi, [pool_base]
    mov     rdi, [aexp_pool]
    mov     ecx, 33
    rep     movsb
    mov     rbx, 33                   ; next export header offset
    mov     rbp, [pool_base]
    mov     r12, [handle_grid]
    mov     r13, [aexp_grid]
    xor     r14d, r14d
.cell:
    cmp     r14, [grid_cells]
    jae     .done
    mov     r15d, [r12 + r14 * 4]
    mov     eax, r15d
    test    eax, eax
    jz      .store
    cmp     eax, [space_handle]
    je      .store
    mov     r9d, eax
    imul    r9d, 0x9e3779b9
.probe:
    and     r9, [aexp_map_mask]
    mov     r10, r9
    shl     r10, 4
    add     r10, [aexp_map]
    mov     rax, [aexp_epoch]
    cmp     [r10], rax
    jne     .new
    cmp     [r10 + 8], r15d
    je      .found
    inc     r9
    jmp     .probe
.new:
    mov     [r10], rax
    mov     [r10 + 8], r15d
    mov     ecx, r15d
    shr     ecx, 24                   ; bounded visual byte length
    add     ecx, VISUAL_HEADER
    lea     rax, [rbx + rcx]
    cmp     rax, POOL_LIMIT
    jae     .full                     ; never publish an out-of-range handle
    mov     esi, r15d
    and     esi, HANDLE_OFFSET_MASK
    lea     rsi, [rbp + rsi - VISUAL_HEADER]
    mov     r11, [aexp_pool]
    add     r11, rbx                  ; export header for this unique visual
    mov     rdi, r11
    add     rbx, rcx
    rep     movsb                    ; preserves symbol, attrs and visual bytes
    ; True-color direct output uses the RGB of xterm-authored colors too;
    ; NONE bg stays absent. Default fg matches direct bridge's light gray.
    mov     rdi, [r11 + 8]
    cmp     rdi, NONE
    jne     .foreground
    mov     edi, 0xcccccc
.foreground:
    mov     esi, edi
    call    audio_tint
    cmp     edi, esi
    setne   byte [aexp_different]
    mov     [r11 + 8], rdi
    mov     rdi, [r11 + 16]
    cmp     rdi, NONE
    je      .mapped
    mov     esi, edi
    call    audio_tint
    cmp     edi, esi
    setne   dl
    or      [aexp_different], dl
    mov     [r11 + 16], rdi
.mapped:
    inc     qword [aexp_unique]
    movzx   eax, byte [aexp_different]
    add     [aexp_changed], rax
    mov     rax, r11
    sub     rax, [aexp_pool]
    add     eax, VISUAL_HEADER
    mov     ecx, r15d
    and     ecx, 0xff000000
    or      eax, ecx
    mov     [r10 + 12], eax
    jmp     .store
.found:
    mov     eax, [r10 + 12]
.store:
    mov     [r13 + r14 * 4], eax
    inc     r14
    jmp     .cell
.done:
    mov     rcx, r13
    mov     r8, [aexp_pool]
    pop     r15
    pop     r14
    pop     r13
    pop     r12
    pop     rbp
    pop     rbx
.raw:
    ret
.full:
    lea     rdi, [msg_pool_full]
    mov     esi, msg_pool_full_len
    jmp     fatal

; Read-only observation of the last export, only inside its synchronous callback.
; No audio samples or process memory inspection: three numeric counters.
EXPORT ttfx_asm_audio_export_stats
ttfx_asm_audio_export_stats:
    mov     rax, [aexp_active]
    mov     [rdi], rax
    mov     rax, [aexp_unique]
    mov     [rdi + 8], rax
    mov     rax, [aexp_changed]
    mov     [rdi + 16], rax
    ret

section .tstate
aexp_active:    resq 1
aexp_unique:    resq 1
aexp_changed:   resq 1
aexp_different: resb 1
alignb 8
aexp_grid:      resq 1
aexp_pool:      resq 1
aexp_map:       resq 1
aexp_map_mask:  resq 1
aexp_epoch:     resq 1
