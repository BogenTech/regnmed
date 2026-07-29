// Service worker for the regnmed PWA (docs/portal.md, #48).
//
// EN regel styrer alt her: **hovedboken caches aldri.** Et regnskap
// som viser gamle tall offline er verre enn et som sier at det ikke
// har kontakt. Derfor caches bare app-skallet; alt som går til API-et
// går til nettet, hver gang.
//
// Skallet er nå Vite-bygget (#76), så JS-en og CSS-en har
// innholdshashede navn vi ikke kan liste opp på forhånd. Regelen er
// derfor uttrykt som ADRESSER, ikke som en filliste: /assets/ er
// bygde, uforanderlige filer, og de faste PWA-filene er navngitt.
// Alt annet — hele API-et — går utenom cachen.
//
// Offline-køen for kvitteringsfoto ligger i appen (IndexedDB), ikke
// her: den skal virke selv om denne arbeideren aldri ble installert.

const SHELL = "regnmed-skall-v2";
const SHELL_FILES = [
  "/",
  "/manifest.webmanifest",
  "/icon-192.png",
  "/icon-512.png",
];

// Hører adressen til app-skallet? Bare da får den ligge i cachen.
function erSkall(url) {
  return SHELL_FILES.indexOf(url.pathname) !== -1 || url.pathname.startsWith("/assets/");
}

self.addEventListener("install", function (event) {
  event.waitUntil(
    caches.open(SHELL).then(function (cache) {
      // Bare de faste filene forhåndslastes; de hashede hentes ved
      // første besøk og blir liggende fordi navnet aldri gjenbrukes.
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

  if (!erSkall(url)) {
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
