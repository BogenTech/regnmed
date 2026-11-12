<script module>
  // The derived invoice status from the API (open, overdue, paid,
  // credited, credit_note) in the words a user reads. One place, so the
  // list and the invoice page always agree.
  export const STATUS_LABEL = {
    open: "Ubetalt",
    overdue: "Forfalt",
    paid: "Betalt",
    credited: "Kreditert",
    credit_note: "Kreditnota",
  };
  const STATUS_CLASS = {
    open: "badge-warning badge-soft",
    overdue: "badge-error",
    paid: "badge-success badge-soft",
    credited: "badge-ghost",
    credit_note: "badge-info badge-soft",
  };

  export function daysOverdue(dueDate) {
    return Math.round((new Date().setHours(0, 0, 0, 0) - Date.parse(dueDate + "T00:00")) / 864e5);
  }
</script>

<script>
  let { status, dueDate } = $props();
</script>

<span class="badge badge-sm whitespace-nowrap {STATUS_CLASS[status] || 'badge-ghost'}">
  {STATUS_LABEL[status] || status}{status === "overdue" ? ", " + daysOverdue(dueDate) + " dager" : ""}
</span>
