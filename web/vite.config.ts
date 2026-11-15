import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { otoolsTauriShimPlugin } from 'otools-plugin-sdk/vite';
export default defineConfig({
  base:'./', plugins:[svelte(),otoolsTauriShimPlugin()],
  define:{__OTOOLS_PLUGIN_UUID__:JSON.stringify('otools-releasehub')},
  build:{outDir:'../dist',emptyOutDir:true},
  resolve:{conditions:['browser']},test:{environment:'jsdom',include:['src/**/*.test.ts']}
});
