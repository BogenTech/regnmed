// Theme switching — the cross-site theme contract with regnid: the same
// daisyUI theme names select the same theme blocks (ui/themes.css is a
// copy of regnid's canonical file), so a user's theme feels identical on
// both sites. The *preference* is per site: localStorage only, never
// synced through the IdP or tokens.
//
// Resolution: stored choice → "system" (no attribute; daisyUI decides:
// light, or dark when the OS actively prefers dark).
//
// Loaded from <head> WITHOUT defer so the theme applies before first
// paint. Controls: #theme-toggle cycles system → light → dark;
// #theme-picker offers the full list.
(function () {
  var KEY = "regnmed-theme";
  var CYCLE = ["system", "light", "dark"];
  var ICON = { system: "🖥️", light: "☀️", dark: "🌙" };

  function stored() {
    try { return localStorage.getItem(KEY); } catch (e) { return null; }
  }
  function save(v) {
    try { localStorage.setItem(KEY, v); } catch (e) { /* private mode */ }
  }
  function apply(mode) {
    var root = document.documentElement;
    if (!mode || mode === "system") {
      root.removeAttribute("data-theme");
    } else {
      root.setAttribute("data-theme", mode);
    }
  }

  var current = stored() || "system";
  apply(current); // pre-paint

  window.regnmedTheme = {
    THEMES: ["system", "light", "dark", "regnid", "kontrast", "nord", "corporate", "business", "dim", "sunset"],
    get: function () { return current; },
    set: function (mode) {
      current = mode;
      save(mode);
      apply(mode);
      this.render();
    },
    cycle: function () {
      this.set(CYCLE[(CYCLE.indexOf(current) + 1) % CYCLE.length]);
    },
    render: function () {
      var btn = document.getElementById("theme-toggle");
      var picker = document.getElementById("theme-picker");
      if (btn) {
        btn.textContent = ICON[current] || "🎨";
        var label = "Fargetema: " + current + " (klikk for å bytte)";
        btn.setAttribute("aria-label", label);
        btn.setAttribute("title", label);
      }
      if (picker) {
        for (var i = 0; i < picker.options.length; i++) {
          if (picker.options[i].value === current) picker.selectedIndex = i;
        }
      }
    },
  };
})();
