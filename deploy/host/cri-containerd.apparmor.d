# Rendered from containerd v2.3.4 contrib/apparmor/template.go with the host's
# own values (abi/3.0, tunables/global, abstractions/base, daemon profile
# "unconfined", no rootlesskit) — i.e. byte-for-byte what containerd itself
# loads — PLUS the two peer rules marked ADDED below.
#
# Why: Ubuntu sets kernel.apparmor_restrict_unprivileged_unconfined=1, so the
# kernel converts an unprivileged unconfined change_profile into a STACK. A
# process the kubelet exec's into a container (a readiness probe) therefore
# carries the label `cri-containerd.apparmor.d//&unconfined`, while the
# container's own processes carry the bare name. containerd's rules name only
# the bare profile, so signals between them are DENIED. On kernel 7.0.0-38 this
# was logged but rarely enforced; on 7.0.0-42 it hits every unprivileged
# container. PostgreSQL uses SIGURG for latch wakeups (PG16+), so the denial
# deadlocks startup/checkpointer and also prevents reaping backends.
#
# Upstream: containerd/containerd#14485 (open, no release yet), k3s-io/k3s#13625.
# This is NOT a loosening: it permits a container's processes to signal peers of
# THEIR OWN container that the kernel happened to label as a stack.
#
# containerd's LoadDefaultProfile skips loading when a profile of this name is
# already present, so this host copy wins.

abi <abi/3.0>,

#include <tunables/global>


profile cri-containerd.apparmor.d flags=(attach_disconnected,mediate_deleted) {

  #include <abstractions/base>


  network,
  capability,
  file,
  umount,
  # Host (privileged) processes may send signals to container processes.
  signal (receive) peer=unconfined,
  # runc may send signals to container processes.
  signal (receive) peer=runc,
  # crun may send signals to container processes.
  signal (receive) peer=crun,
  # Manager may send signals to container processes.
  signal (receive) peer=unconfined,
  # Container processes may send signals amongst themselves.
  signal (send,receive) peer=cri-containerd.apparmor.d,
  # ADDED: same container, stacked label (see header).
  signal (send,receive) peer=cri-containerd.apparmor.d//&unconfined,

  deny @{PROC}/* w,   # deny write for all files directly in /proc (not in a subdir)
  # deny write to files not in /proc/<number>/** or /proc/sys/**
  deny @{PROC}/{[^1-9/],[^1-9/][^0-9/],[^1-9s/][^0-9y/][^0-9s/],[^1-9/][^0-9/][^0-9/][^0-9/]*}/** w,
  deny @{PROC}/sys/[^k]** w,  # deny /proc/sys except /proc/sys/k* (effectively /proc/sys/kernel)
  deny @{PROC}/sys/kernel/{?,??,[^s][^h][^m]**} w,  # deny everything except shm* in /proc/sys/kernel/
  deny @{PROC}/sysrq-trigger rwklx,
  deny @{PROC}/kcore rwklx,

  deny mount,

  deny /sys/[^f]*/** wklx,
  deny /sys/f[^s]*/** wklx,
  deny /sys/fs/[^c]*/** wklx,
  deny /sys/fs/c[^g]*/** wklx,
  deny /sys/fs/cg[^r]*/** wklx,
  deny /sys/firmware/** rwklx,
  deny /sys/devices/virtual/powercap/** rwklx,
  deny /sys/kernel/security/** rwklx,

  # allow processes within the container to trace each other,
  # provided all other LSM and yama setting allow it.
  ptrace (trace,tracedby,read,readby) peer=cri-containerd.apparmor.d,
  # ADDED: same container, stacked label (see header).
  ptrace (trace,tracedby,read,readby) peer=cri-containerd.apparmor.d//&unconfined,
}
