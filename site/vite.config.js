import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { defineConfig } from 'vite';

// The page, Terms and Privacy, plus the installer at /install as plain text (vercel.json does the same in production).
const installer = {
  name: 'installer',
  configureServer(server) {
    server.middlewares.use(serveInstaller);
  },
  configurePreviewServer(server) {
    server.middlewares.use(serveInstaller);
  },
};

async function serveInstaller(req, res, next) {
  if (req.url?.split('?')[0] !== '/install') return next();
  res.setHeader('Content-Type', 'text/plain; charset=utf-8');
  res.end(await readFile(resolve(import.meta.dirname, 'public/install.sh')));
}

export default defineConfig({
  appType: 'mpa',
  plugins: [installer],
  build: {
    rolldownOptions: {
      input: {
        main: resolve(import.meta.dirname, 'index.html'),
        terms: resolve(import.meta.dirname, 'terms.html'),
        privacy: resolve(import.meta.dirname, 'privacy.html'),
      },
    },
  },
  server: { port: 4173 },
  preview: { port: 4173 },
});
