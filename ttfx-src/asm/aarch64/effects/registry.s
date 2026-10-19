// effects/registry.s - the effects this build implements (asm/effects/
// registry.asm).
//
// effect_table[id] = (build, next_frame); a zero entry means "not ported",
// and Rust keeps that effect. A ported effect's file defines HAVE_<name>;
// a file that doesn't is a stub and its row stays zero. Ids are in
// effects/ids.inc (EffectCommand variants in alphabetical order).
//
// build: the effect's __init__ + build(), reading its config through
//        [effect_config]. May FAIL.
// next_frame -> w9 = 1 when the effect produced a frame, 0 when done
//        (the effect's next_frame without ctx.frame(); the engine paces,
//        advances the clock and renders).

.include "effects/beams.s"
.include "effects/binarypath.s"
.include "effects/blackhole.s"
.include "effects/bouncyballs.s"
.include "effects/bubbles.s"
.include "effects/burn.s"
.include "effects/colorshift.s"
.include "effects/crumble.s"
.include "effects/decrypt.s"
.include "effects/errorcorrect.s"
.include "effects/expand.s"
.include "effects/fireworks.s"
.include "effects/highlight.s"
.include "effects/laseretch.s"
.include "effects/matrix.s"
.include "effects/middleout.s"
.include "effects/orbittingvolley.s"
.include "effects/overflow.s"
.include "effects/pour.s"
.include "effects/print.s"
.include "effects/rain.s"
.include "effects/randomsequence.s"
.include "effects/rings.s"
.include "effects/scattered.s"
.include "effects/slice.s"
.include "effects/slide.s"
.include "effects/smoke.s"
.include "effects/spotlights.s"
.include "effects/spray.s"
.include "effects/swarm.s"
.include "effects/sweep.s"
.include "effects/synthgrid.s"
.include "effects/thunderstorm.s"
.include "effects/unstable.s"
.include "effects/vhstape.s"
.include "effects/waves.s"
.include "effects/wipe.s"

.macro EFFECT_ROW name
    .ifdef HAVE_\name
    .quad   \name\()_build, \name\()_next_frame
    .else
    .quad   0, 0
    .endif
.endm

    RELRO
    .balign 8
effect_table:
    EFFECT_ROW beams           // 0
    EFFECT_ROW binarypath      // 1
    EFFECT_ROW blackhole       // 2
    EFFECT_ROW bouncyballs     // 3
    EFFECT_ROW bubbles         // 4
    EFFECT_ROW burn            // 5
    EFFECT_ROW colorshift      // 6
    EFFECT_ROW crumble         // 7
    EFFECT_ROW decrypt         // 8
    EFFECT_ROW errorcorrect    // 9
    EFFECT_ROW expand          // 10
    EFFECT_ROW fireworks       // 11
    EFFECT_ROW highlight       // 12
    EFFECT_ROW laseretch       // 13
    EFFECT_ROW matrix          // 14
    EFFECT_ROW middleout       // 15
    EFFECT_ROW orbittingvolley // 16
    EFFECT_ROW overflow        // 17
    EFFECT_ROW pour            // 18
    EFFECT_ROW print           // 19
    EFFECT_ROW rain            // 20
    EFFECT_ROW randomsequence  // 21
    EFFECT_ROW rings           // 22
    EFFECT_ROW scattered       // 23
    EFFECT_ROW slice           // 24
    EFFECT_ROW slide           // 25
    EFFECT_ROW smoke           // 26
    EFFECT_ROW spotlights      // 27
    EFFECT_ROW spray           // 28
    EFFECT_ROW swarm           // 29
    EFFECT_ROW sweep           // 30
    EFFECT_ROW synthgrid       // 31
    EFFECT_ROW thunderstorm    // 32
    EFFECT_ROW unstable        // 33
    EFFECT_ROW vhstape         // 34
    EFFECT_ROW waves           // 35
    EFFECT_ROW wipe            // 36
