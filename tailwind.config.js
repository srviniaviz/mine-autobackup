/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        sans: ["Inter", "Segoe UI", "system-ui", "sans-serif"],
        mono: ["Cascadia Mono", "Consolas", "monospace"]
      },
      boxShadow: {
        block: "0 0 0 2px rgba(0,0,0,.45), inset 0 0 0 1px rgba(255,255,255,.08)"
      }
    }
  },
  plugins: []
};
