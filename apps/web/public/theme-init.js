// Runs before the app bundle to avoid a flash of the wrong theme.
// Keys and values must match src/lib/preferences.tsx.
(function () {
  try {
    var root = document.documentElement;
    var theme = localStorage.getItem('harness.theme') || 'system';
    var dark = theme === 'dark' || (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
    root.classList.toggle('dark', dark);
    var density = localStorage.getItem('harness.density');
    if (density === 'compact' || density === 'comfortable') {
      root.dataset.density = density;
    }
    var locale = localStorage.getItem('harness.locale');
    if (locale === 'de' || locale === 'en') {
      root.lang = locale;
    }
  } catch (e) {
    // Storage can be unavailable (private mode); defaults apply.
  }
})();
