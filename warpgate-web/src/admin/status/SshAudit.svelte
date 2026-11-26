<script lang="ts">
    import { Alert } from '@sveltestrap/sveltestrap'
    import { api, type LogEntry } from 'admin/lib/api'
    import { adminPermissions } from 'admin/lib/store'
    import EmptyState from 'common/EmptyState.svelte'
    import { stringifyError } from 'common/errors'
    import { onMount } from 'svelte'
    import { link } from 'svelte-spa-router'

    const sampleSize = 300
    const groups = {
        SshSessionLinked1: { name: 'Sessions', label: 'Session linked' },
        SshSessionDeadline1: { name: 'Sessions', label: 'Session expired' },
        SshExecRequested1: { name: 'Commands', label: 'Exec requested' },
        SshShellCommandObserved1: { name: 'Commands', label: 'Shell command' },
        SshSftpOperation1: { name: 'Transfers', label: 'SFTP operation' },
        SshSftpAuditGap1: { name: 'Transfers', label: 'SFTP audit gap' },
        SshSftpUnanswered1: { name: 'Transfers', label: 'SFTP interrupted' },
        TicketApprovalRequested1: {
            name: 'Approvals',
            label: 'Approval requested',
        },
        TicketApprovalDecided1: {
            name: 'Approvals',
            label: 'Approval decided',
        },
    } as const
    const tabs = [
        'All activity',
        'Sessions',
        'Commands',
        'Transfers',
        'Approvals',
    ] as const
    type Tab = (typeof tabs)[number]
    type EventType = keyof typeof groups

    let entries: LogEntry[] = $state([])
    let tab: Tab = $state('All activity')
    let loading = $state(false)
    let refreshedAt: Date | undefined = $state()
    let error: string | undefined = $state()

    function field(entry: LogEntry, name: string): string {
        const value: unknown = entry.values?.[name]
        return typeof value === 'string' ? value.replace(/^"|"$/g, '') : ''
    }

    const activity = $derived(
        entries.flatMap(entry => {
            const type = field(entry, '_type')
            return Object.hasOwn(groups, type)
                ? [{ entry, type: type as EventType }]
                : []
        }),
    )
    const visible = $derived(
        tab === 'All activity'
            ? activity
            : activity.filter(({ type }) => groups[type].name === tab),
    )
    const sessions = $derived(
        activity.filter(({ type }) => type === 'SshSessionLinked1').length,
    )
    const transferBytes = $derived(
        activity
            .filter(({ type }) => type === 'SshSftpOperation1')
            .reduce(
                (total, { entry }) =>
                    total + (Number(field(entry, 'bytes_transferred')) || 0),
                0,
            ),
    )
    const auditGaps = $derived(
        activity.filter(({ type }) => type === 'SshSftpAuditGap1').length,
    )

    function describe(entry: LogEntry, type: EventType): string {
        if (type === 'SshSftpOperation1') {
            const status = field(entry, 'status')
            const bytes = field(entry, 'bytes_transferred')
            return [
                field(entry, 'operation'),
                field(entry, 'path'),
                field(entry, 'second_path'),
                bytes && `${Number(bytes).toLocaleString()} bytes`,
                status && `status ${status}`,
            ]
                .filter(Boolean)
                .join(' · ')
        }
        return (
            field(entry, 'command') ||
            field(entry, 'reason') ||
            field(entry, 'request_id') ||
            field(entry, 'target_session_id') ||
            field(entry, 'phase')
        )
    }

    async function refresh() {
        if (loading) return
        loading = true
        try {
            entries = await api.getLogs({
                getLogsRequest: { target: 'audit', limit: sampleSize },
            })
            refreshedAt = new Date()
            error = undefined
        } catch (cause) {
            error = await stringifyError(cause)
        } finally {
            loading = false
        }
    }

    onMount(() => {
        void refresh()
        const timer = setInterval(() => {
            if (document.visibilityState === 'visible') void refresh()
        }, 15000)
        return () => clearInterval(timer)
    })
</script>

<div class="container-max-lg">
    <div class="page-summary-bar">
        <h1>SSH audit</h1>
        <button
            class="btn btn-outline-secondary btn-sm ms-auto"
            type="button"
            disabled={loading}
            onclick={refresh}
        >
            Refresh
        </button>
    </div>

    <p class="text-muted small">
        Latest {sampleSize} audit entries
        {#if refreshedAt}
            · Updated {refreshedAt.toLocaleTimeString()}
        {/if}
    </p>

    {#if error}
        <Alert color="danger">{error}</Alert>
    {/if}

    <dl class="summary mb-4">
        <div class="card p-3">
            <dt>Target sessions linked</dt>
            <dd>{sessions.toLocaleString()}</dd>
        </div>
        <div class="card p-3">
            <dt>SFTP bytes transferred</dt>
            <dd>{transferBytes.toLocaleString()}</dd>
        </div>
        <div class="card p-3">
            <dt>SFTP audit gaps</dt>
            <dd class:text-warning={auditGaps > 0}>
                {auditGaps.toLocaleString()}
            </dd>
        </div>
    </dl>

    <nav aria-label="Filter SSH activity" class="d-flex flex-wrap gap-2 mb-3">
        {#each tabs as candidate (candidate)}
            <button
                class="btn btn-sm"
                class:btn-primary={tab === candidate}
                class:btn-outline-secondary={tab !== candidate}
                aria-pressed={tab === candidate}
                type="button"
                onclick={() => (tab = candidate)}
            >
                {candidate}
            </button>
        {/each}
    </nav>

    {#if visible.length}
        <div class="table-responsive">
            <table class="table align-middle">
                <thead>
                    <tr>
                        <th scope="col">Time</th>
                        <th scope="col">Event</th>
                        <th scope="col">Details</th>
                        <th scope="col">Session</th>
                    </tr>
                </thead>
                <tbody>
                    {#each visible as { entry, type } (entry.id)}
                        <tr>
                            <td class="text-nowrap text-muted small">
                                <time datetime={entry.timestamp.toISOString()}>
                                    {entry.timestamp.toLocaleString()}
                                </time>
                            </td>
                            <td>
                                <span class="fw-semibold"
                                    >{groups[type].label}</span
                                >
                                {#if entry.username}
                                    <span class="small text-muted d-block"
                                        >{entry.username}</span
                                    >
                                {/if}
                            </td>
                            <td class="detail">{describe(entry, type)}</td>
                            <td>
                                {#if entry.sessionId &&
                                    $adminPermissions.sessionsView}
                                    <a
                                        use:link
                                        href={`/status/sessions/${entry.sessionId}`}
                                        >View session</a
                                    >
                                {:else if entry.sessionId}
                                    <code title={entry.sessionId}
                                        >{entry.sessionId.slice(0, 8)}</code
                                    >
                                {/if}
                            </td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    {:else if !loading && !error}
        <EmptyState title="No matching audit activity" />
    {/if}
</div>

<style>
    .summary {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(11rem, 1fr));
        gap: 0.75rem;
    }

    dt {
        font-size: 0.875rem;
        font-weight: 500;
    }

    dd {
        margin-bottom: 0;
        font-size: 1.75rem;
        font-variant-numeric: tabular-nums;
    }

    .detail {
        overflow-wrap: anywhere;
        max-width: 28rem;
    }
</style>
