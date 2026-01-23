<script lang="ts">
    import { base } from "$app/paths";
    import logotype from "$lib/assets/logotype.png";
    import illustrationSpecRip from "$lib/assets/illustration-spec-rip.png";
    import illustrationRunningRobot from "$lib/assets/illustration-running-robot.png";
    import illustrationCowboy from "$lib/assets/illustration-cowboy.png";

    const brewCommand = "brew install rocket-tycoon/tap/manifest";

    let copied = $state(false);
    let highlightInstall = $state(false);

    function scrollToInstall() {
        const topSection = document.getElementById("top");
        if (topSection) {
            topSection.scrollIntoView({ behavior: "smooth" });
            setTimeout(() => {
                highlightInstall = true;
                setTimeout(() => (highlightInstall = false), 2000);
            }, 500);
        }
    }

    async function copyBrewCommand() {
        await navigator.clipboard.writeText(brewCommand);
        copied = true;
        setTimeout(() => (copied = false), 2000);
    }
</script>

<div class="marketing-page">
    <nav class="top-nav">
        <a href="{base}/docs/" class="nav-button nav-button--docs">Read the Docs</a>
    </nav>
    <main>
        <section id="top" class="hero">
            <img src={logotype} alt="Manifest" class="hero-logotype" />
            <h1>
                Living documentation for feature-driven <span class="nowrap"
                    >product engineering</span
                >
            </h1>
            <p class="hero-subheadline">
                Keep humans and agents in the loop with always up-to-date
                context of your app
            </p>
        </section>

        <section class="install-section">
            <div class="install-box" class:highlight={highlightInstall}>
                <div class="install-wrapper">
                    <code class="install-command">{brewCommand}</code>
                    <button
                        class="copy-button"
                        onclick={copyBrewCommand}
                        aria-label="Copy brew command"
                    >
                        {#if copied}
                            <svg
                                width="18"
                                height="18"
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                            >
                                <polyline points="20 6 9 17 4 12" />
                            </svg>
                        {:else}
                            <svg
                                width="18"
                                height="18"
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                            >
                                <rect
                                    width="14"
                                    height="14"
                                    x="8"
                                    y="8"
                                    rx="2"
                                    ry="2"
                                />
                                <path
                                    d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"
                                />
                            </svg>
                        {/if}
                    </button>
                </div>
            </div>
        </section>

        <section id="pain-points" class="pain-points">
            <h2 class="section-title">
                Yesterday's workflows don't work for AI's pace and complexity
            </h2>
            <div class="pain-cards">
                <div class="pain-card">
                    <div class="pain-card-illustration-container">
                        <img
                            src={illustrationSpecRip}
                            alt="RIP Spec document"
                            class="pain-card-illustration pain-card-illustration--spec"
                        />
                    </div>
                    <h3>Specs die on day one</h3>
                    <p>
                        AI makes it easy to iterate... away from your spec which
                        is out of date after the first commit.
                    </p>
                </div>
                <div class="pain-card">
                    <div class="pain-card-illustration-container">
                        <img
                            src={illustrationRunningRobot}
                            alt="Robot running fast"
                            class="pain-card-illustration pain-card-illustration--robot"
                        />
                    </div>
                    <h3>Change happens too fast</h3>
                    <p>
                        Agents are quick, managing them via Jira or Github
                        Issues is the path to maddness.
                    </p>
                </div>
                <div class="pain-card">
                    <div class="pain-card-illustration-container">
                        <img
                            src={illustrationCowboy}
                            alt="Cowboy coding"
                            class="pain-card-illustration pain-card-illustration--cowboy"
                        />
                    </div>
                    <h3>Agents get confused</h3>
                    <p>
                        Without context of past work or a map for where their
                        going, agents sling the wrong thing.
                    </p>
                </div>
            </div>
        </section>

        <section class="solution">
            <h2 class="section-title">
                Manifest is a better way to define and track coding agents' work
            </h2>
            <div class="value-props">
                <div class="value-prop">
                    <h3>
                        <span class="value-icon">
                            <!-- Implemented state icon (filled circle with checkmark) -->
                            <svg
                                width="20"
                                height="20"
                                viewBox="0 0 16 16"
                                fill="none"
                                xmlns="http://www.w3.org/2000/svg"
                            >
                                <circle
                                    cx="8"
                                    cy="8"
                                    r="7"
                                    fill="var(--state-implemented)"
                                />
                                <path
                                    d="M4.5 8L7 10.5L11.5 5.5"
                                    stroke="var(--background)"
                                    stroke-width="1.5"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                />
                            </svg>
                        </span>
                        Features, not tickets
                    </h3>
                    <p>
                        Features describe what your system does (or will do),
                        living capabilities that evolve, not work items to close
                        and forget.
                    </p>
                </div>
                <div class="value-prop">
                    <h3>
                        <span class="value-icon">
                            <!-- Clock icon -->
                            <svg
                                width="20"
                                height="20"
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="var(--accent-green)"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                            >
                                <circle cx="12" cy="12" r="10" />
                                <polyline points="12 6 12 12 16 14" />
                            </svg>
                        </span>
                        Activity log for agent work
                    </h3>
                    <p>
                        Track what agents built, see their commits, and
                        understand how each feature was implemented.
                    </p>
                </div>
                <div class="value-prop">
                    <h3>
                        <span class="value-icon">
                            <!-- Lucide bot icon (Claude orange) -->
                            <svg
                                width="20"
                                height="20"
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="#E07A3E"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                            >
                                <path d="M12 8V4H8" />
                                <rect
                                    width="16"
                                    height="12"
                                    x="4"
                                    y="8"
                                    rx="2"
                                />
                                <path d="M2 14h2" />
                                <path d="M20 14h2" />
                                <path d="M15 13v2" />
                                <path d="M9 13v2" />
                            </svg>
                        </span>
                        Built for AI agents
                    </h3>
                    <p>
                        MCP server lets any agent read and update features.
                        <a href="{base}/docs/getting-started"
                            >Add the Claude Code plugin</a
                        >
                        for slash commands and enhanced workflows.
                    </p>
                </div>
            </div>
        </section>

        <section class="cta-section">
            <a class="cta-button cta-button--secondary" href="{base}/docs/"
                >Read the Docs</a
            >
            <button class="cta-button" onclick={scrollToInstall}
                >Install Now</button
            >
        </section>
    </main>

    <footer class="site-footer">
        <div class="footer-content">
            <p>&copy; 2026 Rocket Tycoon</p>
        </div>
    </footer>
</div>

<style>
    .marketing-page {
        min-height: 100vh;
        display: flex;
        flex-direction: column;
        font-size: 112%;
    }

    /* Top Navigation */
    .top-nav {
        position: fixed;
        top: 0;
        right: 0;
        display: flex;
        gap: 0.5em;
        padding: 1em;
        z-index: 100;
    }

    .nav-button {
        padding: 0.4em 0.9em;
        font-size: 0.8em;
        font-weight: 500;
        text-decoration: none;
        background: transparent;
        border: 1.5px solid #fff;
        color: #fff;
        transition:
            background 0.15s ease,
            color 0.15s ease,
            border-color 0.15s ease;
    }

    .nav-button--docs:hover {
        background: var(--state-implemented);
        border-color: var(--state-implemented);
        color: var(--background);
    }

    .nav-button--app:hover {
        background: var(--accent-green);
        border-color: var(--accent-green);
        color: var(--background);
    }

    .nowrap {
        white-space: nowrap;
    }

    /* Sections */
    main {
        flex: 1;
        display: flex;
        flex-direction: column;
    }

    section {
        padding: 3em 1.5em;
    }

    .section-title {
        font-size: 1.5em;
        font-weight: 600;
        text-align: center;
        margin: 0 0 1.5em;
        color: var(--foreground);
    }

    .pain-points .section-title {
        margin-bottom: 0.75em;
    }

    /* Hero */
    .hero {
        text-align: center;
        padding: 1.375em 1.5em 1em;
        max-width: 68.75em;
        margin: 0 auto;
    }

    /* Install Section */
    .install-section {
        display: flex;
        justify-content: center;
        align-items: center;
        padding: 0.5em 1.5em 0em;
    }

    .hero-logotype {
        width: 20.25em;
        height: auto;
        margin: 0 auto 0.75em;
        display: block;
    }

    .hero h1 {
        font-size: 2em;
        font-weight: 700;
        line-height: 1.2;
        margin: 0 0 0.25em;
        color: var(--foreground);
    }

    .hero-subheadline {
        font-size: 1em;
        line-height: 1.5;
        color: var(--foreground-muted);
        margin: 0 0 0.5em;
    }

    .install-wrapper {
        display: inline-flex;
        align-items: stretch;
        border: 1px solid var(--border-muted);
    }

    .install-command {
        display: inline-block;
        padding: 0.625em 1.25em;
        font-family: "IBM Plex Mono", ui-monospace, monospace;
        font-size: 0.875em;
        color: var(--foreground);
        background: rgba(255, 255, 255, 0.02);
        user-select: all;
        cursor: text;
    }

    .copy-button {
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 0 0.75em;
        background: var(--background);
        border: none;
        border-left: 1px solid var(--border-muted);
        color: var(--foreground-muted);
        cursor: pointer;
        transition: color 0.15s ease;
    }

    .copy-button:hover {
        color: var(--foreground);
    }

    .install-box {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 0.75em;
        padding: 1em;
        border: 2px solid transparent;
        transition: border-color 0.3s ease;
    }

    .install-box.highlight {
        border-color: var(--accent-green);
    }

    /* Pain Points */
    .pain-points {
        background: var(--background);
        padding-top: 2.5em;
        padding-bottom: 1.5em;
    }

    .pain-cards {
        max-width: 62.5em;
        margin: 0 auto;
        display: grid;
        grid-template-columns: repeat(3, 1fr);
        gap: 0;
    }

    .pain-card {
        background: var(--background);
        padding: 1em;
        text-align: center;
        position: relative;
    }

    .pain-card:not(:last-child)::after {
        content: "";
        position: absolute;
        right: 0;
        top: 1.25em;
        bottom: 0;
        width: 1px;
        background: var(--border-muted);
    }

    .pain-card h3 {
        font-size: 1.125em;
        font-weight: 600;
        margin: 0 0 0.5em;
        color: var(--foreground);
    }

    .pain-card p {
        font-size: 0.875em;
        line-height: 1.5;
        color: var(--foreground-muted);
        margin: 0;
    }

    .pain-card-illustration-container {
        height: 8.75em;
        display: flex;
        align-items: flex-end;
        justify-content: center;
        margin-bottom: 0.625em;
    }

    .pain-card-illustration {
        max-width: 10em;
        height: auto;
        display: block;
    }

    .pain-card-illustration--spec {
        max-width: 11.875em;
        transform: translateY(0.9375em);
    }

    .pain-card-illustration--robot {
        max-width: 11.875em;
        transform: translateY(0.5em);
    }

    .pain-card-illustration--cowboy {
        max-width: 10.9375em;
    }

    /* Solution / Value Props */
    .solution {
        background: var(--background);
        padding-top: 1.5em;
        padding-bottom: 3.25em;
    }

    .value-props {
        max-width: 62.5em;
        margin: 0 auto;
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(18.75em, 1fr));
        gap: 2em;
    }

    .value-prop {
        text-align: center;
    }

    .value-prop h3 {
        font-size: 1.125em;
        font-weight: 600;
        margin: 0 0 0.75em;
        color: var(--foreground);
        display: inline-flex;
        align-items: center;
        gap: 0.5em;
    }

    .value-prop h3 .value-icon {
        display: flex;
        align-items: center;
    }

    .value-prop p {
        font-size: 0.9375em;
        line-height: 1.6;
        color: var(--foreground-muted);
        margin: 0;
    }

    .value-prop p strong {
        color: var(--foreground);
    }

    .value-prop p code {
        padding: 0.125em 0.375em;
        font-size: 0.8125em;
        border: 1px solid var(--border-muted);
        white-space: nowrap;
    }

    .value-prop p a {
        color: var(--accent-green);
        text-decoration: none;
    }

    .value-prop p a:hover {
        text-decoration: underline;
    }

    /* CTA Section */
    .cta-section {
        display: flex;
        justify-content: center;
        gap: 1em;
        background: var(--background);
        padding: 0.5em 1.5em 4em;
    }

    .cta-button {
        display: inline-block;
        font-size: 1em;
        font-weight: 500;
        color: var(--accent-green);
        background: transparent;
        border: 2px solid var(--accent-green);
        padding: 0.75em 2em;
        cursor: pointer;
        text-decoration: none;
        transition:
            background 0.15s ease,
            color 0.15s ease;
    }

    .cta-button:hover {
        background: var(--accent-green);
        color: var(--background);
    }

    .cta-button--secondary {
        color: var(--state-implemented);
        border-color: var(--state-implemented);
    }

    .cta-button--secondary:hover {
        background: var(--state-implemented);
        color: var(--background);
    }

    /* Footer */
    .site-footer {
        background: var(--background);
        border-top: 1px solid var(--border-muted);
        padding: 1.5em;
    }

    .footer-content {
        max-width: 62.5em;
        margin: 0 auto;
        text-align: center;
    }

    .footer-content p {
        margin: 0;
        font-size: 0.875em;
        color: var(--foreground-subtle);
    }

    /* Responsive */
    @media (max-width: 48em) {
        section {
            padding: 2em 1em;
        }

        .hero {
            padding: 1em 1em 2em;
        }

        .hero-logotype {
            width: 17.5em;
        }

        .hero h1 {
            font-size: 1.5em;
        }

        .hero-subheadline {
            font-size: 0.9375em;
        }

        .section-title {
            font-size: 1.375em;
            margin-bottom: 1.5em;
        }

        .pain-cards {
            grid-template-columns: 1fr;
        }

        .pain-card {
            padding: 1.25em;
            border-right: none;
            border-bottom: 1px solid var(--border-muted);
        }

        .pain-card:last-child {
            border-bottom: none;
        }

        .value-prop {
            padding: 1.25em;
        }
    }
</style>
