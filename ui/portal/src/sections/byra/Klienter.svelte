<script>
  import Card from "../../components/Card.svelte";

  let { clients } = $props();
</script>

<Card title="Klienter">
  <table class="table table-sm">
    <thead>
      <tr><th>Selskap</th><th>Type</th><th>Fra</th><th>Status</th></tr>
    </thead>
    <tbody>
      {#each clients as e (e.engagement_id)}
        <tr>
          <td>{e.company}</td>
          <td>{e.kind}</td>
          <td>{e.valid_from}</td>
          <td>{e.valid_to ? "avsluttet " + e.valid_to : "aktivt"}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</Card>
