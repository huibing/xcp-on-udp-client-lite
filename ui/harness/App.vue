<template>
  <div id="xcp-app"><Experiment /></div>
</template>

<script setup>
import Experiment from '../src/components/Experiment.vue'
</script>

<style>
html, body, #app { margin: 0; height: 100%; }
#xcp-app {
  height: 100vh;
  --exp-bg: #090909; --exp-bg2: #060606; --exp-bg3: #0d0d0d; --exp-bg4: #0a0a0a; --exp-bg5: #111111;
  --exp-fg: #00cc44; --exp-fg-bright: #00ff66; --exp-fg-mid: #00aa33;
  --exp-fg-dim: #556644; --exp-fg-dim2: #558844; --exp-fg-dim3: #446633; --exp-fg-dim4: #335522;
  --exp-border: #003311; --exp-border2: #002211; --exp-border3: #001a09;
  --exp-hover: #003311; --exp-ok-bg: #003322; --exp-err-bg: #330011;
  --exp-working-bg: #005522; --exp-primary-bg: #004433; --exp-cal-hover: #2a2a3f;
  background: var(--exp-bg);
}
</style>
