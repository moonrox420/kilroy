/**
 * Tailwind 4 ships its own PostCSS plugin and embeds an Oxide engine, so
 * the standalone `tailwindcss` + `autoprefixer` pair from the v3 era is
 * gone. One plugin entry, done.
 */
export default {
  plugins: {
    "@tailwindcss/postcss": {},
  },
};
