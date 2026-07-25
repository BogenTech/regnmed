// Service worker for the regnmed PWA (docs/portal.md, #48).
//
// EN regel styrer alt her: **hovedboken caches aldri.** Et regnskap
// som viser gamle tall offline er verre enn et som sier at det ikke
// har kontakt. Derfor caches bare app-skallet (HTML, JS, CSS, ikoner);
// alt som går til API-et går til nettet, hver gang.
//
// Offline-køen for kvitteringsfoto ligger i app.js (IndexedDB), ikke
// her: den skal virke selv om denne arbeideren aldri ble installert.

const SHELL = "regnmed-skall-v1";
const SHELL_FILES = [
  "/",
  "/app.js",
  "/app.css",
  "/theme.js",
  "/manifest.webmanifest",
  "/icon-192.png",
  "/icon-512.png",
];

self.addEventListener("install", function (event) {
  event.waitUntil(
    caches.open(SHELL).then(function (cache) {
      return cache.addAll(SHELL_FILES);
    }).then(function () {
      return self.skipWaiting();
    })
  );
});

self.addEventListener("activate", function (event) {
  // En ny versjon av skallet rydder bort den gamle med det samme.
  event.waitUntil(
    caches.keys().then(function (names) {
      return Promise.all(names.filter(function (n) { return n !== SHELL; })
        .map(function (n) { return caches.delete(n); }));
    }).then(function () {
      return self.clients.claim();
    })
  );
});

self.addEventListener("fetch", function (event) {
  var request = event.request;
  if (request.method !== "GET") return;
  var url = new URL(request.url);
  if (url.origin !== self.location.origin) return;

  var isShell = SHELL_FILES.indexOf(url.pathname) !== -1;
  if (!isShell) {
    // Data. Nettet eller ingenting — aldri en lagret kopi av tall.
    return;
  }
  event.respondWith(
    // Nett først, så en oppdatert app vinner over den lagrede, og
    // cachen bare trår til når telefonen er uten dekning.
    fetch(request)
      .then(function (response) {
        var copy = response.clone();
        caches.open(SHELL).then(function (cache) { cache.put(request, copy); });
        return response;
      })
      .catch(function () {
        return caches.match(request).then(function (hit) {
          return hit || caches.match("/");
        });
      })
  );
});
