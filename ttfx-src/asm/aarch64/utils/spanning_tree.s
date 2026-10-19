// utils/spanning_tree.s - spanning-tree generators (src/utils/spanning_tree.rs):
// PrimsSimple, PrimsWeighted, RecursiveBacktracker and BreadthFirst.
//
// EffectCharacter.links: 16 bytes per slot in [st_links], up to four linked
// slots (a character only links to its grid neighbors) kept ascending, each
// stored as slot + 1 so a zero word ends the list. Ascending slot order is
// ascending character_id, the canonical order BreadthFirst walks.
//
// Each generator is a single instance in .tstate (no effect runs two of the
// same kind). Every constructor draws its starting character exactly like
// default_starting_char when none is given.
//
// ST_WEIGHTS (defs.inc) is randint(0, 99)'s range.

    .text

// st_init: allocate the links table once per run. Only characters that
// exist when a generator starts (the input and fill characters, which carry
// the neighbor records) are ever linked.
st_init:
    LDX     x9, st_links
    cbnz    x9, st_init.done
    str     x30, [sp, #-16]!
    LDW     w0, char_count
    lsl     x0, x0, #4
    bl      alloc
    STX     x9, st_links
    ldr     x30, [sp], #16
st_init.done:
    ret

// st_insert(w0=slot, w1=other): add other to slot's ascending link list,
// unless present. Clobbers x9, x3, x2, x4.
st_insert:
    LDX     x4, st_links
    add     x4, x4, w0, uxtw #4
    add     w2, w1, #1                  // the stored word
    mov     w3, #0
st_insert.find:
    cmp     w3, #4
    b.hs    st_insert.done              // full (cannot happen for grid links)
    ldr     w9, [x4, w3, uxtw #2]
    cbz     w9, st_insert.put
    cmp     w9, w2
    b.eq    st_insert.done
    b.hi    st_insert.shift
    add     w3, w3, #1
    b       st_insert.find
st_insert.shift:
    // insert at w3: swap the new word through the tail
    str     w2, [x4, w3, uxtw #2]
    mov     w2, w9
    add     w3, w3, #1
    cmp     w3, #4
    b.hs    st_insert.done
    ldr     w9, [x4, w3, uxtw #2]
    cbnz    w9, st_insert.shift
st_insert.put:
    str     w2, [x4, w3, uxtw #2]
st_insert.done:
    ret

// link_characters(w0=a, w1=b): EffectCharacter._link, both directions.
// Clobbers x9, x3, x2, x4 (x0 and x1 come back as their low words).
link_characters:
    str     x30, [sp, #-16]!
    bl      st_insert
    mov     w9, w0
    mov     w0, w1
    mov     w1, w9
    bl      st_insert
    mov     w9, w0
    mov     w0, w1
    mov     w1, w9
    ldr     x30, [sp], #16
    ret

// st_has_links(w0=slot) -> w9 nonzero when the character has links.
// Clobbers x3.
st_has_links:
    LDX     x9, st_links
    ubfiz   x3, x0, #4, #32
    ldr     w9, [x9, x3]
    ret

// st_neighbors(w0=slot, w1=limit to text, x2=u32 out[4]) -> w9 = count.
// SpanningTreeGenerator.get_neighbors with unlinked_only: north, east, south,
// west, kept when inside the text boundary (if limited) and unlinked.
// Clobbers x9, x3, x4, x5, x10, x11.
st_neighbors:
    LDX     x4, ch_nbr
    add     x4, x4, w0, uxtw #4
    mov     w9, #0                      // count
    mov     w5, #0                      // direction
st_neighbors.next:
    cmp     w5, #4
    b.hs    st_neighbors.done
    ldr     w3, [x4, w5, uxtw #2]
    add     w5, w5, #1
    cmn     w3, #1                      // NONE
    b.eq    st_neighbors.next
    cbz     w1, st_neighbors.links
    LDX     x10, ch_irow
    ldrsw   x10, [x10, w3, uxtw #2]
    LDX     x11, text_bottom
    cmp     x10, x11
    b.lt    st_neighbors.next
    LDX     x11, text_top
    cmp     x10, x11
    b.gt    st_neighbors.next
    LDX     x10, ch_icol
    ldrsw   x10, [x10, w3, uxtw #2]
    LDX     x11, text_left
    cmp     x10, x11
    b.lt    st_neighbors.next
    LDX     x11, text_right
    cmp     x10, x11
    b.gt    st_neighbors.next
st_neighbors.links:
    LDX     x10, st_links
    ubfiz   x11, x3, #4, #32
    ldr     w11, [x10, x11]
    cbnz    w11, st_neighbors.next
    str     w3, [x2, w9, uxtw #2]
    add     w9, w9, #1
    b       st_neighbors.next
st_neighbors.done:
    ret

// st_starting_char(w0=within text) -> w9 = slot: default_starting_char.
st_starting_char:
    str     x30, [sp, #-16]!
    mov     w1, w0
    mov     w0, #0
    bl      canvas_random_coord
    mov     x1, x9
    bl      char_at_input_coord
    ldr     x30, [sp], #16
    cmn     w9, #1                      // NONE
    b.eq    st_starting_char.missing
    ret
st_starting_char.missing:
    FAIL    msg_no_starting_char

// st_start(w0=starting slot or NONE, w1=within text) -> w9 = slot.
st_start:
    cmn     w0, #1                      // NONE
    b.ne    st_start.given
    mov     w0, w1
    b       st_starting_char
st_start.given:
    mov     w9, w0
    ret

// st_array(x0=entries per character) -> x9: a zeroed u32 array sized for
// every current character slot times the factor.
st_array:
    LDW     w9, char_count
    mul     x0, x0, x9
    lsl     x0, x0, #2
    add     x0, x0, #64
    b       alloc

// ------------------------------------------------------------ PrimsSimple

// ps_new(w0=limit to text): PrimsSimple::new(None, limit).
ps_new:
    stp     x19, x30, [sp, #-16]!
    mov     w19, w0
    STW     w0, ps_limit
    bl      st_init
    mov     w0, #NONE
    mov     w1, w19
    bl      st_start
    mov     w19, w9
    mov     x0, #1
    bl      st_array
    STX     x9, ps_order
    str     w19, [x9]
    mov     x3, #1
    STX     x3, ps_order_count
    mov     x0, #2
    bl      st_array
    STX     x9, ps_edges
    str     w19, [x9]
    mov     x3, #1
    STX     x3, ps_edge_count
    STB     wzr, ps_complete
    ldp     x19, x30, [sp], #16
    ret

// ps_step: PrimsSimple.step (complete flips only when the edge list is
// already empty on entry).
ps_step:
    sub     sp, sp, #64
    stp     x19, x21, [sp, #32]
    stp     x22, x30, [sp, #48]
    // [sp] and [sp+16]: two u32[4] neighbor lists
    LDX     x1, ps_edge_count
    cbz     x1, ps_step.complete
    mov     x0, #0
    bl      rng_randrange
    // current = edge_chars.remove(idx)
    LDX     x3, ps_edges
    ldr     w19, [x3, x9, lsl #2]
    LDX     x2, ps_edge_count
    sub     x2, x2, #1
    STX     x2, ps_edge_count
ps_step.shift:
    cmp     x9, x2
    b.hs    ps_step.shifted
    add     x4, x9, #1
    ldr     w5, [x3, x4, lsl #2]
    str     w5, [x3, x9, lsl #2]
    mov     x9, x4
    b       ps_step.shift
ps_step.shifted:
    mov     w0, w19
    LDW     w1, ps_limit
    mov     x2, sp
    bl      st_neighbors
    cbz     w9, ps_step.done
    mov     w22, w9                     // unlinked neighbor count
    mov     x0, #0
    mov     w1, w9
    bl      rng_randrange
    ldr     w21, [sp, x9, lsl #2]       // next_char
    sub     w22, w22, #1                // one removed
    mov     w0, w19
    mov     w1, w21
    bl      link_characters
    LDX     x9, ps_order_count
    LDX     x3, ps_order
    str     w21, [x3, x9, lsl #2]
    add     x9, x9, #1
    STX     x9, ps_order_count
    cbz     w22, ps_step.next_neighbors
    LDX     x9, ps_edge_count
    LDX     x3, ps_edges
    str     w19, [x3, x9, lsl #2]
    add     x9, x9, #1
    STX     x9, ps_edge_count
ps_step.next_neighbors:
    mov     w0, w21
    LDW     w1, ps_limit
    add     x2, sp, #16
    bl      st_neighbors
    cbz     w9, ps_step.done
    LDX     x9, ps_edge_count
    LDX     x3, ps_edges
    str     w21, [x3, x9, lsl #2]
    add     x9, x9, #1
    STX     x9, ps_edge_count
    b       ps_step.done
ps_step.complete:
    mov     w3, #1
    STB     w3, ps_complete
ps_step.done:
    ldp     x19, x21, [sp, #32]
    ldp     x22, x30, [sp, #48]
    add     sp, sp, #64
    ret

// ps_run -> x9 = char_link_order (u32 slots), x2 = count: step until
// complete.
ps_run:
    str     x30, [sp, #-16]!
ps_run.loop:
    LDB     w9, ps_complete
    cbnz    w9, ps_run.done
    bl      ps_step
    b       ps_run.loop
ps_run.done:
    LDX     x9, ps_order
    LDX     x2, ps_order_count
    ldr     x30, [sp], #16
    ret

// ---------------------------------------------------------- PrimsWeighted

// pw_new(w0=limit to text): PrimsWeighted::new(None, limit) - the starting
// character, one randint(0, 99) weight per input and fill character from top
// to bottom, left to right, then the starting character's weighted links.
pw_new:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    mov     w19, w0
    STW     w0, pw_limit
    bl      st_init
    mov     w0, #NONE
    mov     w1, w19
    bl      st_start
    mov     w19, w9                     // starting char
    LDW     w0, char_count
    bl      alloc
    STX     x9, pw_weights
    mov     w0, #(FILTER_INPUT | FILTER_INNER_FILL | FILTER_OUTER_FILL)
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
pw_new.weigh:
    cbz     x22, pw_new.weighed
    mov     x0, #0
    mov     x1, #(ST_WEIGHTS - 1)
    bl      rng_randint
    ldr     w3, [x21]
    LDX     x2, pw_weights
    strb    w9, [x2, x3]
    add     x21, x21, #4
    sub     x22, x22, #1
    b       pw_new.weigh
pw_new.weighed:
    // bucket w holds up to 4 links per character: [pw_buckets] + w * cap * 8
    LDW     w9, char_count
    lsl     x9, x9, #2
    add     x9, x9, #4
    lsl     x9, x9, #3
    STX     x9, pw_bucket_bytes
    mov     x3, #ST_WEIGHTS
    mul     x0, x9, x3
    bl      alloc
    STX     x9, pw_buckets
    mov     x0, #1
    bl      st_array
    STX     x9, pw_order
    str     w19, [x9]
    mov     x3, #1
    STX     x3, pw_order_count
    STB     wzr, pw_complete
    mov     w0, w19
    bl      pw_add_links
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// PW_BUCKET: x9 = the base of weight w3's bucket. Clobbers x16.
.macro PW_BUCKET
    mov     w9, w3
    LDX     x16, pw_bucket_bytes
    mul     x9, x9, x16
    LDX     x16, pw_buckets
    add     x9, x9, x16
.endm

// pw_add_links(w0=slot): add_weighted_links - one (slot, neighbor) link per
// unlinked neighbor into the neighbor's weight bucket.
pw_add_links:
    sub     sp, sp, #48
    stp     x19, x21, [sp, #16]
    stp     x22, x30, [sp, #32]
    // [sp]: u32[4] neighbors
    mov     w19, w0
    LDW     w1, pw_limit
    mov     x2, sp
    bl      st_neighbors
    mov     w21, w9
    mov     w22, #0
pw_add_links.next:
    cmp     w22, w21
    b.hs    pw_add_links.done
    ldr     w4, [sp, w22, uxtw #2]      // neighbor
    LDX     x3, pw_weights
    ldrb    w3, [x3, x4]
    PW_BUCKET
    ADRG    x2, pw_counts
    ldr     x5, [x2, x3, lsl #3]
    orr     x4, x19, x4, lsl #32        // char_a low, char_b high
    str     x4, [x9, x5, lsl #3]
    add     x5, x5, #1
    str     x5, [x2, x3, lsl #3]
    // bts [pw_nonempty], weight
    ADRG    x10, pw_nonempty
    lsr     x5, x3, #6
    ldr     x11, [x10, x5, lsl #3]
    mov     x12, #1
    lsl     x12, x12, x3
    orr     x11, x11, x12
    str     x11, [x10, x5, lsl #3]
    add     w22, w22, #1
    b       pw_add_links.next
pw_add_links.done:
    ldp     x19, x21, [sp, #16]
    ldp     x22, x30, [sp, #32]
    add     sp, sp, #48
    ret

// pw_lowest -> x3 = the lowest nonempty weight with Z clear, or Z set when
// none. Clobbers x9.
pw_lowest:
    LDX     x9, pw_nonempty
    rbit    x3, x9
    clz     x3, x3
    cbnz    x9, pw_lowest.found
    LDX     x9, pw_nonempty + 8
    rbit    x3, x9
    clz     x3, x3
    add     x3, x3, #64
pw_lowest.found:
    tst     x9, x9                      // Z set only when both words are empty
    ret

// pw_step: PrimsWeighted.step with get_lowest_weight_link inlined: pop a
// random link of the lowest weight until one reaches an unlinked character.
pw_step:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    bl      pw_lowest
    b.eq    pw_step.complete
pw_step.pop:
    mov     w21, w3                     // weight
    ADRG    x2, pw_counts
    ldr     x1, [x2, x3, lsl #3]
    mov     x0, #0
    bl      rng_randrange
    mov     x5, x9                      // idx
    mov     w3, w21
    PW_BUCKET
    ldr     x22, [x9, x5, lsl #3]       // the link: char_a low, char_b high
    ADRG    x2, pw_counts
    ldr     x4, [x2, x3, lsl #3]
    sub     x4, x4, #1
    str     x4, [x2, x3, lsl #3]
    cbnz    x4, pw_step.remove
    // btr [pw_nonempty], weight
    ADRG    x10, pw_nonempty
    lsr     x11, x3, #6
    ldr     x12, [x10, x11, lsl #3]
    mov     x13, #1
    lsl     x13, x13, x3
    bic     x12, x12, x13
    str     x12, [x10, x11, lsl #3]
pw_step.remove:
    // links_at_weight.remove(idx)
    cmp     x5, x4
    b.hs    pw_step.removed
    add     x10, x5, #1
    ldr     x11, [x9, x10, lsl #3]
    str     x11, [x9, x5, lsl #3]
    mov     x5, x10
    b       pw_step.remove
pw_step.removed:
    lsr     x0, x22, #32
    bl      st_has_links
    cbz     w9, pw_step.found
    bl      pw_lowest
    b.ne    pw_step.pop
    b       pw_step.complete
pw_step.found:
    mov     w0, w22
    lsr     x1, x22, #32
    mov     w19, w1
    bl      link_characters
    LDX     x9, pw_order_count
    LDX     x3, pw_order
    str     w19, [x3, x9, lsl #2]
    add     x9, x9, #1
    STX     x9, pw_order_count
    mov     w0, w19
    bl      pw_add_links
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret
pw_step.complete:
    mov     w3, #1
    STB     w3, pw_complete
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// pw_run: step until complete.
pw_run:
    str     x30, [sp, #-16]!
pw_run.loop:
    LDB     w9, pw_complete
    cbnz    w9, pw_run.done
    bl      pw_step
    b       pw_run.loop
pw_run.done:
    ldr     x30, [sp], #16
    ret

// --------------------------------------------------- RecursiveBacktracker

// rb_new(w0=limit to text): RecursiveBacktracker::new(None, limit).
rb_new:
    stp     x19, x30, [sp, #-16]!
    mov     w19, w0
    STW     w0, rb_limit
    bl      st_init
    mov     w0, #NONE
    mov     w1, w19
    bl      st_start
    mov     w19, w9
    STW     w9, rb_current
    mov     x0, #1
    bl      st_array
    STX     x9, rb_order
    str     w19, [x9]
    mov     x3, #1
    STX     x3, rb_order_count
    mov     x0, #1
    bl      st_array
    STX     x9, rb_stack
    str     w19, [x9]
    mov     x3, #1
    STX     x3, rb_stack_count
    STB     wzr, rb_complete
    ldp     x19, x30, [sp], #16
    ret

// rb_step: RecursiveBacktracker.step - link a random unvisited neighbor of
// the current character and push it, or pop back.
rb_step:
    sub     sp, sp, #48
    stp     x19, x21, [sp, #16]
    str     x30, [sp, #32]
    // [sp]: u32[4] neighbors
    LDX     x9, rb_stack_count
    cbz     x9, rb_step.complete
    LDW     w19, rb_current
    mov     w0, w19
    LDW     w1, rb_limit
    mov     x2, sp
    bl      st_neighbors
    cbz     w9, rb_step.backtrack
    mov     w0, w9
    bl      rng_below                   // choice
    ldr     w21, [sp, x9, lsl #2]
    mov     w0, w19
    mov     w1, w21
    bl      link_characters
    LDX     x9, rb_order_count
    LDX     x3, rb_order
    str     w21, [x3, x9, lsl #2]
    add     x9, x9, #1
    STX     x9, rb_order_count
    LDX     x9, rb_stack_count
    LDX     x3, rb_stack
    str     w21, [x3, x9, lsl #2]
    add     x9, x9, #1
    STX     x9, rb_stack_count
    STW     w21, rb_current
    b       rb_step.done
rb_step.backtrack:
    LDX     x9, rb_stack_count
    sub     x9, x9, #1
    STX     x9, rb_stack_count
    cbz     x9, rb_step.done
    LDX     x3, rb_stack
    add     x3, x3, x9, lsl #2
    ldur    w3, [x3, #-4]
    STW     w3, rb_current
    b       rb_step.done
rb_step.complete:
    mov     w3, #1
    STB     w3, rb_complete
rb_step.done:
    ldp     x19, x21, [sp, #16]
    ldr     x30, [sp, #32]
    add     sp, sp, #48
    ret

// rb_run -> x9 = char_link_order, x2 = count: step until complete.
rb_run:
    str     x30, [sp, #-16]!
rb_run.loop:
    LDB     w9, rb_complete
    cbnz    w9, rb_run.done
    bl      rb_step
    b       rb_run.loop
rb_run.done:
    LDX     x9, rb_order
    LDX     x2, rb_order_count
    ldr     x30, [sp], #16
    ret

// ----------------------------------------------------------- BreadthFirst

// bf_new(w0=starting slot or NONE, w1=limit to text) -> w9 = the
// starting character. BreadthFirst::new. The frontier and every later layer
// live in one queue: the frontier is [bf_head, bf_tail).
bf_new:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    mov     w21, w0
    mov     w22, w1
    bl      st_init
    mov     w0, w21
    mov     w1, w22
    bl      st_start
    mov     w19, w9
    STW     w9, bf_start
    LDW     w0, char_count
    bl      alloc
    STX     x9, bf_explored
    mov     w3, #1
    strb    w3, [x9, x19]
    mov     x0, #1
    bl      st_array
    STX     x9, bf_queue
    str     w19, [x9]
    STX     xzr, bf_head
    mov     x3, #1
    STX     x3, bf_tail
    STB     wzr, bf_complete
    mov     w9, w19
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// bf_step -> x9 = explored_last_step (u32 slots), x2 = count.
// BreadthFirst.step: every frontier character's unexplored links, in
// frontier order and ascending id within each, become the next frontier.
// (Anything in the frontier or in new_edges is already explored, so the
// explored test covers Rust's three membership checks.)
// Clobbers x9, x3, x2, x4, x5, x10, x11.
bf_step:
    stp     x19, x21, [sp, #-32]!
    str     x22, [sp, #16]
    LDX     x19, bf_head
    LDX     x21, bf_tail                // end of the frontier
    cmp     x19, x21
    b.eq    bf_step.complete
    mov     x22, x21                    // new tail
    LDX     x4, bf_queue
    LDX     x5, bf_explored
bf_step.position:
    cmp     x19, x21
    b.hs    bf_step.layered
    ldr     w9, [x4, x19, lsl #2]
    add     x19, x19, #1
    LDX     x10, st_links
    add     x9, x10, x9, lsl #4
    mov     w3, #0
bf_step.link:
    cmp     w3, #4
    b.hs    bf_step.position
    ldr     w2, [x9, w3, uxtw #2]
    add     w3, w3, #1
    cbz     w2, bf_step.position
    sub     w2, w2, #1
    ldrb    w11, [x5, x2]
    cbnz    w11, bf_step.link
    mov     w11, #1
    strb    w11, [x5, x2]
    str     w2, [x4, x22, lsl #2]
    add     x22, x22, #1
    b       bf_step.link
bf_step.layered:
    STX     x21, bf_head
    STX     x22, bf_tail
    add     x9, x4, x21, lsl #2
    sub     x2, x22, x21
    b       bf_step.done
bf_step.complete:
    mov     w3, #1
    STB     w3, bf_complete
    mov     x2, #0
bf_step.done:
    ldr     x22, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

    .section .rodata
STRING msg_no_starting_char, "Unable to find a starting character."

    TSTATE
    .balign 8
st_links:           .skip 8
ps_order:           .skip 8
ps_order_count:     .skip 8
ps_edges:           .skip 8
ps_edge_count:      .skip 8
ps_limit:           .skip 4
ps_complete:        .skip 1
    .balign 8
rb_order:           .skip 8
rb_order_count:     .skip 8
rb_stack:           .skip 8
rb_stack_count:     .skip 8
rb_current:         .skip 4
rb_limit:           .skip 4
rb_complete:        .skip 1
    .balign 8
bf_queue:           .skip 8
bf_explored:        .skip 8
bf_head:            .skip 8
bf_tail:            .skip 8
bf_start:           .skip 4
bf_complete:        .skip 1
    .balign 8
pw_weights:         .skip 8
pw_buckets:         .skip 8
pw_bucket_bytes:    .skip 8
pw_counts:          .skip 8 * ST_WEIGHTS
pw_nonempty:        .skip 8 * 2
pw_order:           .skip 8
pw_order_count:     .skip 8
pw_limit:           .skip 4
pw_complete:        .skip 1

    .text
