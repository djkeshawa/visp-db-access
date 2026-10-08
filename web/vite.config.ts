import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: { proxy: { '/api': 'http://localhost:8080' } },
  build: {
    manifest: true,
    rolldownOptions: {
      preserveEntrySignatures: false,
      output: {
        strictExecutionOrder: true,
        codeSplitting: {
          includeDependenciesRecursively: false,
          groups: [
            { name: 'editor', test: /@codemirror|@lezer/, priority: 30 },
            { name: 'formatter', test: /sql-formatter/, priority: 30 },
            {
              name: 'grid',
              test: /@tanstack\/(?:react-virtual|virtual-core)/,
              priority: 30,
            },
            { name: 'primitives', test: /@radix-ui/, priority: 20 },
            { name: 'vendor', test: /node_modules/, priority: 10 },
          ],
        },
      },
    },
  },
});
