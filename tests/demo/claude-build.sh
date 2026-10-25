#!/bin/sh
# Demo mode (src/demo.rs): what a Claude Code session building WEB-160 looks
# like, replayed into the ticket's Claude tab. No real claude runs.
# Looks like Claude Code 2.1: mascot header, prompts on a grey bar, a rule
# above and below the input box, the mode line under it.
O='\033[38;2;215;119;87m'; D='\033[2m'; B='\033[1m'; G='\033[38;2;78;186;101m'
R='\033[0m'; P='\033[48;2;34;66;44m'; M='\033[48;2;84;38;38m'; C='\033[38;2;150;150;150m'
U='\033[48;2;72;74;80m'; Y='\033[38;2;232;176;72m'
cols=$(stty size 2>/dev/null | cut -d' ' -f2); [ -n "$cols" ] || cols=100
say() { printf "$@"; }
rule() { i=0; while [ $i -lt "$cols" ]; do printf '─'; i=$((i + 1)); done; printf '\n'; }
asked() { printf "${U}❯ %-$((cols - 2))s${R}\n\n" "$1"; }
spin() { printf "\r\033[2K${O}✻${R} $1… ${D}(esc to interrupt)${R}"; sleep "$2"; printf "\r\033[2K"; }
say "${O} ▐▛███▜▌${R}   ${B}Claude Code${R} ${D}v2.1.310${R}\n"
say "${O}▝▜█████▛▘${R}  ${D}Opus 5.5 · Claude Max${R}\n"
say "${O}  ▘▘ ▝▝${R}    ${D}~/orbit/work/WEB-160${R}\n\n"
asked "Build WEB-160: Pin a dashboard to the sidebar. Follow the plan on the ticket."
spin "Reading the ticket" 1.2
spin "Exploring" 1
say "● ${B}Read${R}(src/sidebar/Sidebar.tsx)\n"
say "  └  Read ${B}142${R} lines\n\n"
spin "Writing" 1.6
say "● ${B}Update${R}(src/sidebar/Sidebar.tsx)\n"
say "  └  Updated ${B}src/sidebar/Sidebar.tsx${R} with ${B}7${R} additions and ${B}1${R} removal\n"
say "       40     const dashboards = useDashboards();\n"
say "       41 ${P} +   const pinned = usePinnedDashboards();                      ${R}\n"
say "       42 ${P} +   const rest = dashboards.filter((d) => !pinned.has(d.id));  ${R}\n"
say "       43     return (\n"
say "       44       <nav className=\"sidebar\">\n"
say "       45 ${M} -       <DashboardList items={dashboards} />                   ${R}\n"
say "       45 ${P} +       {pinned.size > 0 && (                                  ${R}\n"
say "       46 ${P} +         <DashboardList title=\"Pinned\" items={[...pinned]} /> ${R}\n"
say "       47 ${P} +       )}                                                     ${R}\n"
say "       48 ${P} +       <DashboardList items={rest} />                         ${R}\n\n"
spin "Writing" 1.4
say "● ${B}Update${R}(src/dashboards/DashboardMenu.tsx)\n"
say "  └  Updated ${B}src/dashboards/DashboardMenu.tsx${R} with ${B}4${R} additions\n\n"
say "● ${B}Update Todos${R}\n"
say "  └  ${G}√${R} ${D}Store pinned dashboards in the user's settings${R}\n"
say "     ${G}√${R} ${D}Show pinned dashboards at the top of the sidebar${R}\n"
say "     ${G}√${R} ${D}Pin and unpin from the dashboard menu${R}\n"
say "     ☐ Tests, then open the PR\n\n"
spin "Testing" 2.2
say "● ${B}Bash${R}(pnpm test sidebar)\n"
say "  └  ${G} PASS ${R} src/sidebar/Sidebar.test.tsx\n"
say "       ${G}√${R} lists pinned dashboards first (31 ms)\n"
say "       ${G}√${R} hides the pinned section when nothing is pinned (12 ms)\n"
say "       ${G}√${R} unpinning moves a dashboard back (18 ms)\n"
say "     Tests: ${G}${B}6 passed${R}, 6 total\n\n"
spin "Committing" 1.6
say "● ${B}Bash${R}(git push -u origin web-160-pin-dashboards && gh pr create --fill)\n"
say "  └  https://github.com/acme/web/pull/321\n\n"
say "● Done. Pinned dashboards show at the top of the sidebar, pinning is one\n"
say "  click in the dashboard menu, and the six sidebar tests pass. The PR is\n"
say "  ready for review.\n\n"
say "${D}✻ Baked for 2m 14s · done 11:48 AM${R}\n\n"
rule
say "❯ \n"
rule
say "  ${Y}⏵⏵ auto mode on${R} ${D}(shift+tab to cycle)${R}"
exec sleep 86400
