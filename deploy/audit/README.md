# Warpgate audit archive

This OpenTofu and Terraform configuration provisions a private object bucket
for Warpgate session recordings and archived journal events. Run it with the
storage account's infrastructure identity. Use a separate runtime identity
scoped to the bucket or the relevant object prefix.

```hcl
backend               = "r2"
cloudflare_account_id = "00000000000000000000000000000000"
bucket_name           = "example-warpgate-audit"
retention_days       = 365
loki_endpoint        = "https://loki.internal.example.net"
```

Set `backend = "s3"` and `aws_region` for AWS S3. Use the provider's
environment or workload credentials to apply the infrastructure. Review the
plan before applying the retention rule to production data. The module
outputs connection settings and the collector configuration, without
creating or storing access keys in state.

```sh
tofu init
tofu plan
tofu apply
tofu output -raw vector_config > vector.toml
tofu output -json recordings_s3
```

## Run the collector

Install Vector with the rendered configuration on the Warpgate gateway and
on each target whose SSH journal should be archived. Grant its service
account read access to the journal and a persistent, writable
`/var/lib/vector`. Use Warpgate's `log.format: json` option for structured
fields. Set `journal_units` to the appropriate systemd services when they
differ from the defaults. Configure the target's SSH audit correlation as
described in [SSH audit correlation](../../docs/ssh-audit.md).

For AWS, use the `s3_collector_policy` output to scope the collector role.
For R2, provide the collector's bucket-scoped S3 credentials through
`AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`. The runtime R2 token should
have object access and no permission to administer bucket locks. Pass
credentials through the service's secret mechanism.

Vector persists journal checkpoints and queued batches in its data
directory. The archive contains gzip-compressed newline-delimited JSON
beneath `audit/YYYY/MM/DD/`. The optional Loki sink is intended for the
monitoring dashboard.

## Configure recordings

In Warpgate's config UI, enable recordings and select **S3**. Use the
`recordings_s3` output to fill in the bucket, prefix, region, endpoint and
path-style mode. Supply a separate runtime identity for Warpgate. The
recording scratch directory needs persistent storage. Keep Warpgate's
`log.retention` longer than the object retention period to avoid cleanup
attempts while recording objects are retained. The UI's connection test
writes and deletes a probe, so a retention policy can block that delete.
Verify the recording path with a test session and a successful playback.

## Retention guarantees

| Backend | Guarantee for committed objects |
| --- | --- |
| S3 | Object Lock in `COMPLIANCE` mode protects each version during its retention period. Preserve version IDs in investigations, since subsequent writes can create new versions. |
| R2 | Bucket Lock protects against deletion and overwrites while the lock rule applies. Control-plane administrators who can change the lock rule are part of the trust boundary. |

The lock applies after a successful upload. Audit generation, the local
journal, and the upload buffer are within the producing system's trust
boundary. A host that is already compromised can suppress events before
upload. Keep archive administration separate from the gateway and target
hosts, and monitor the collector for upload failures and backpressure.
See [AWS Object Lock](https://docs.aws.amazon.com/AmazonS3/latest/userguide/object-lock.html)
and [Cloudflare R2 Bucket Locks](https://developers.cloudflare.com/r2/buckets/bucket-locks/).

## Monitor activity

The gateway includes **Status → SSH audit** in its administration UI. The
page refreshes every 15 seconds when visible and summarizes the newest
300 audit entries, with links to sessions and filters for commands,
transfers and approvals.

When `loki_endpoint` is configured, import
[Warpgate SSH audit](grafana/warpgate-ssh-audit.json) in Grafana and select
the Loki data source receiving the Vector stream. The dashboard shows
SSH activity, SFTP transfer volume, audit gaps and the underlying events.
Keep Loki behind an authenticated private endpoint or gateway. Its buffer
can drop new events when full so a Loki outage does not stall archival.
Use the retained object archive for investigations that require complete
historical evidence.
