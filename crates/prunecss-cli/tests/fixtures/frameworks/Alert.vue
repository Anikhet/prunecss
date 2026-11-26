<template>
  <p :class="{ 'text-danger': hasError }" :class="[isBig ? 'big' : 'small']">Alert</p>
</template>
