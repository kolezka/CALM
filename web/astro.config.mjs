import { defineConfig } from 'astro/config';

export default defineConfig({
  site: 'https://calm.raqz.link',
  output: 'static',
  build: { inlineStylesheets: 'auto' },
});
