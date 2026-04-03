import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  base: "/web/",
  build: {
    target: "esnext",
    outDir: "dist",
    emptyOutDir: true,
    minify: false,
    cssCodeSplit: false,
    modulePreload: false,
    rollupOptions: {
      output: {
        entryFileNames: "app.js",
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: (assetInfo) => {
          if (assetInfo.names?.some((name) => name.endsWith(".css"))) return "app.css";
          return "assets/[name]-[hash][extname]";
        },
      },
    },
  },
});
