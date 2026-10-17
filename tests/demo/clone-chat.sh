#!/bin/sh
# Demo mode (src/demo.rs): the clone's chat filing and driving an
# initiative, replayed into the clone panel. No real claude runs.
O='\033[38;2;215;119;87m'; D='\033[2m'; B='\033[1m'; G='\033[38;2;78;186;101m'
R='\033[0m'; C='\033[38;2;150;150;150m'
say() { printf "$@"; }
spin() { printf "\r\033[2K${O}*${R} $1… ${D}(esc to interrupt)${R}"; sleep "$2"; printf "\r\033[2K"; }
say "${C}> Ship saved views for dashboards: per-user saved filters, shareable${R}\n"
say "${C}  links, default view. Robo builds, I review.${R}\n\n"
spin "Researching" 1.2
say "⏺ I'll check how dashboards keep filters today and what is on the board.\n\n"
say "⏺ ${B}Bash${R}(orbit ctl tickets all)\n"
say "  └  18 tickets · nothing on saved views yet\n\n"
say "⏺ ${B}Read${R}(web: src/dashboards/filters.ts)\n"
say "  └  Read ${B}96${R} lines\n\n"
spin "Planning" 1.4
say "⏺ Four sub-tickets, in build order. Sharing needs the stored views first.\n\n"
say "⏺ ${B}linear - create_issue${R} (MCP)\n"
say "  └  WEB-170 Saved views on dashboards\n"
say "     WEB-171 Store filters per user\n"
say "     WEB-172 Pick a default view\n"
say "     WEB-173 Share a view by link   ${D}blocked by WEB-171${R}\n"
say "     WEB-174 The views menu\n\n"
say "⏺ ${B}Bash${R}(orbit ctl mission WEB-170 2)\n"
say "  └  driving WEB-170, 2 at a time\n\n"
say "⏺ Driving ${B}WEB-170${R}:\n"
say "  ${G}→${R} WEB-171 to Robo, PR web#322 is up for your review\n"
say "  ${G}→${R} WEB-172 to Robo, building\n"
say "  ${D}·${R} WEB-173 waits for WEB-171 to merge\n"
say "  ${D}·${R} WEB-174 next, once one of them lands\n\n"
say "  I'll tell you when a PR needs your review.\n\n"
say "${D}╭─────────────────────────────────────────────────╮${R}\n"
say "${D}│${R} >                                               ${D}│${R}\n"
say "${D}╰─────────────────────────────────────────────────╯${R}\n"
exec sleep 86400
