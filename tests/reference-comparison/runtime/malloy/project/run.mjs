import {PostgresConnection} from "@malloydata/db-postgres";
import {SingleConnectionRuntime} from "@malloydata/malloy";

const connection = new PostgresConnection({
  name: "postgres",
  host: process.env.REFERENCE_DATABASE_HOST,
  port: Number(process.env.REFERENCE_DATABASE_PORT),
  username: process.env.REFERENCE_DATABASE_USER,
  password: process.env.REFERENCE_DATABASE_PASSWORD,
  databaseName: process.env.REFERENCE_DATABASE_NAME,
  ssl: false,
});
const runtime = new SingleConnectionRuntime({connection});

try {
  const result = await runtime
    .loadQuery(`
      source: orders is postgres.table('commerce.orders') extend {
        measure: total_revenue is amount.sum()
      }

      run: orders -> {
        aggregate: total_revenue
      }
    `)
    .run();
  process.stdout.write(`${JSON.stringify(result.data.toObject())}\n`);
} finally {
  await runtime.shutdown();
}
