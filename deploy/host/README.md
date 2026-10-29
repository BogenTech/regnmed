# Vertsoppsett utenfor Flux

Flux styrer det som lever *i* clusteret. Dette er det som må ligge på
**noden selv**, og som derfor må gjøres om igjen når en node settes opp
på nytt.

## `cri-containerd.apparmor.d`

AppArmor-profilen containerd gir hver container. Vi legger inn vår egen
kopi fordi containerds innebygde mangler to regler — se hendelsen
2026-10-29 i [../../docs/deploy.md](../../docs/deploy.md) og
[containerd#14485](https://github.com/containerd/containerd/issues/14485).

```sh
for n in luke mando andor joda obiwan; do
  scp deploy/host/cri-containerd.apparmor.d $n:/tmp/
  ssh $n 'sudo cp /tmp/cri-containerd.apparmor.d /etc/apparmor.d/ \
          && sudo apparmor_parser -r -W /etc/apparmor.d/cri-containerd.apparmor.d'
done
```

containerds `LoadDefaultProfile` hopper over innlastingen når en profil
med samme navn alt er lastet, så kopien vår vinner. Filen i
`/etc/apparmor.d/` overlever omstart.

**Sjekk at den virker:**

```sh
ssh <node> 'sudo dmesg | grep -c "apparmor=\"DENIED\".*signal"'   # skal ikke vokse
```

**Fjern den igjen** når containerd har sluppet en rettelse: slett filen,
kjør `apparmor_parser -R`, og start containerd — da laster den sin egen
igjen.
