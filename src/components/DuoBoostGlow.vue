<template>
    <div class="duo-boost-wrapper" aria-hidden="true">
        <div class="duo-outer-glow"></div>
        <div class="duo-neon-rim" :class="{ 'top-attached': topAttached }"></div>
    </div>
</template>

<script setup lang="ts">
defineProps<{ topAttached?: boolean }>();
</script>

<style>
@property --duo-ring-angle {
    syntax: '<angle>';
    initial-value: 0deg;
    inherits: false;
}
</style>

<style scoped>
.duo-boost-wrapper {
    --duo-lap: 1.4s;
    position: absolute;
    inset: 0;
    border-radius: inherit;
    pointer-events: none;
    z-index: 3;
    transition: border-radius 260ms ease;
}
.duo-neon-rim,
.duo-outer-glow {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    pointer-events: none;
}
.duo-neon-rim {
    padding: 2px;
    background: conic-gradient(from var(--duo-ring-angle),
        #ffbe59 0deg, #ff736c 55deg, #c375ff 115deg,
        #797dff 180deg, #58d8ff 235deg, #63efba 290deg, #ffbe59 360deg);
    -webkit-mask: linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0);
    -webkit-mask-composite: xor;
    mask: linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0);
    mask-composite: exclude;
    animation: duo-ring-spin var(--duo-lap) linear infinite;
}
.duo-neon-rim.top-attached {
    padding-top: 0;
}
.duo-outer-glow {
    box-shadow: 0 0 4px 1px #b9a4ffcc,
                0 0 10px 2px #9a76ff8f,
                0 0 14px 3px #8264ff66,
                0 0 22px 4px #7660ff33;
}
@keyframes duo-ring-spin {
    to { --duo-ring-angle: 360deg; }
}
@media (prefers-reduced-motion: reduce) {
    .duo-neon-rim { animation: none; }
    .duo-boost-wrapper { transition: none; }
}
</style>
