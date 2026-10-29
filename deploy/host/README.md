# Vertsoppsett utenfor Flux

Flux styrer det som lever *i* clusteret. Dette er det som må ligge på
**noden selv**, og som derfor må gjøres om igjen når en node settes opp
på nytt.

## `cri-containerd.apparmor.d` — AppArmor-profilen containerd gir containere

Hendelsen 2026-10-29 (fire timer, test og prod — hele analysen står i
[../../docs/deploy.md](../../docs/deploy.md)) skyldtes at containerds
innebygde profil mangler to regler. Ubuntu setter
`kernel.apparmor_restrict_unprivileged_unconfined=1`, så kjernen gjør et
uprivilegert ukonfinert `change_profile` om til en **stabel**: en prosess
kubelet exec-er inn i containeren (en readiness-probe) bærer
`cri-containerd.apparmor.d//&unconfined`, mens containerens egne
prosesser bærer det bare navnet. Malen tillater signaler bare til det
bare navnet, så signaler mellom dem AVSLÅS. PostgreSQL vekker latcher
med SIGURG (PG16+), og uten SIGTERM kan postmaster ikke høste backender.

Rettelsen er containerds **egen mal for din containerd-versjon**, rendret
med vertens egne verdier, pluss disse to linjene:

```
signal (send,receive) peer=cri-containerd.apparmor.d//&unconfined,
ptrace (trace,tracedby,read,readby) peer=cri-containerd.apparmor.d//&unconfined,
```

Malen hentes fra containerd-kilden (`contrib/apparmor`) for den versjonen
noden faktisk kjører — derfor ligger det **ingen ferdig profil i dette
repoet**. En rendret profil er vertsspesifikk, og en kopi fra en annens
maskin er dårlig råd. For vår egen homelab ligger den utrullede kopien i
`bisand/homelab` under `nodes/common/cri-containerd.apparmor.d`, sammen
med utrullingsløkken og verifiseringen.

containerds `LoadDefaultProfile` hopper over innlastingen når en profil
med samme navn alt er lastet, og `apparmor.service` laster alt i
`/etc/apparmor.d/` ved oppstart — så kopien vinner uten at containerd
konfigureres, og den overlever omstart.

**Dette svekker ingenting**: det tillater signaler mellom prosesser i
samme container som kjernen tilfeldigvis merket som en stabel. De to
alternativene er begge verre — `appArmorProfile: Unconfined` fjerner all
innestenging fra databasen, og `sysctl …=0` slår av herdingen på hele
verten.

**Ingenting i k3s eller Flux legger filen tilbake.** En node som bygges
på nytt kommer opp uten den, og da er hendelsen tilbake. Vår
driftssjekk (`nodes/maintain.sh status` i homelab-repoet) rapporterer
`apparmor=ok|MISSING` per node nettopp derfor.

**Fjern den igjen** når containerd har sluppet en rettelse: slett filen,
kjør `apparmor_parser -R`, og start containerd — da laster den sin egen.
Oppstrøms: [containerd#14485](https://github.com/containerd/containerd/issues/14485)
(åpen, ingen release).
