import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

export default defineConfig({
	plugins: [svelte()],
	clearScreen: false,
	server: {
		port: 5173,
		strictPort: true,
	},
	envPrefix: ["VITE_", "TAURI_"],
	build: {
		target: "esnext",
		minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
		sourcemap: !!process.env.TAURI_DEBUG,
	},
});
