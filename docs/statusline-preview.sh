#!/bin/bash
# Statusline preview - Tokyo Night & Nord with dividers

RESET="\033[0m"
ARR=""
DIV="─"

# ═══════════════════════════════════════════════════════════════════
# TOKYO NIGHT
# ═══════════════════════════════════════════════════════════════════
TN_BLUE="\033[38;2;122;162;247m"
TN_CYAN="\033[38;2;125;207;255m"
TN_PURPLE="\033[38;2;187;154;247m"
TN_MAGENTA="\033[38;2;157;124;216m"
TN_GREEN="\033[38;2;158;206;106m"
TN_ORANGE="\033[38;2;255;158;100m"
TN_TEAL="\033[38;2;42;195;222m"
TN_YELLOW="\033[38;2;224;175;104m"
TN_GRAY="\033[38;2;86;95;137m"
TN_WHITE="\033[38;2;169;177;214m"
TN_BLACK="\033[38;2;26;27;38m"

TN_BG_BLUE="\033[48;2;122;162;247m"
TN_BG_CYAN="\033[48;2;125;207;255m"
TN_BG_PURPLE="\033[48;2;187;154;247m"
TN_BG_MAGENTA="\033[48;2;157;124;216m"
TN_BG_GREEN="\033[48;2;158;206;106m"
TN_BG_ORANGE="\033[48;2;255;158;100m"
TN_BG_TEAL="\033[48;2;42;195;222m"
TN_BG_YELLOW="\033[48;2;224;175;104m"
TN_BG_GRAY="\033[48;2;52;59;88m"
TN_BG_DARK="\033[48;2;26;27;38m"

# ═══════════════════════════════════════════════════════════════════
# NORD
# ═══════════════════════════════════════════════════════════════════
N_BLUE="\033[38;2;136;192;208m"       # #88c0d0 - frost
N_CYAN="\033[38;2;129;161;193m"       # #81a1c1 - frost darker
N_PURPLE="\033[38;2;180;142;173m"     # #b48ead - aurora purple
N_GREEN="\033[38;2;163;190;140m"      # #a3be8c - aurora green
N_ORANGE="\033[38;2;208;135;112m"     # #d08770 - aurora orange
N_TEAL="\033[38;2;143;188;187m"       # #8fbcbb - frost teal
N_YELLOW="\033[38;2;235;203;139m"     # #ebcb8b - aurora yellow
N_RED="\033[38;2;191;97;106m"         # #bf616a - aurora red
N_GRAY="\033[38;2;76;86;106m"         # #4c566a - polar night
N_WHITE="\033[38;2;236;239;244m"      # #eceff4 - snow storm
N_BLACK="\033[38;2;46;52;64m"         # #2e3440 - polar night

N_BG_BLUE="\033[48;2;136;192;208m"
N_BG_CYAN="\033[48;2;129;161;193m"
N_BG_PURPLE="\033[48;2;180;142;173m"
N_BG_GREEN="\033[48;2;163;190;140m"
N_BG_ORANGE="\033[48;2;208;135;112m"
N_BG_TEAL="\033[48;2;143;188;187m"
N_BG_YELLOW="\033[48;2;235;203;139m"
N_BG_GRAY="\033[48;2;59;66;82m"
N_BG_DARK="\033[48;2;46;52;64m"

DIM="\033[2m"

# Tokyo Night dim/muted colors
TN_BLUE="\033[38;2;122;162;247m"
TN_CYAN="\033[38;2;125;207;255m"
TN_PURPLE="\033[38;2;187;154;247m"
TN_MAGENTA="\033[38;2;157;124;216m"
TN_GREEN="\033[38;2;158;206;106m"
TN_ORANGE="\033[38;2;255;158;100m"
TN_TEAL="\033[38;2;42;195;222m"
TN_YELLOW="\033[38;2;224;175;104m"
TN_GRAY="\033[38;2;86;95;137m"
TN_WHITE="\033[38;2;169;177;214m"

# Dim versions (using ANSI dim + color)
TND_BLUE="\033[2;38;2;122;162;247m"
TND_CYAN="\033[2;38;2;125;207;255m"
TND_PURPLE="\033[2;38;2;187;154;247m"
TND_MAGENTA="\033[2;38;2;157;124;216m"
TND_GREEN="\033[2;38;2;158;206;106m"
TND_ORANGE="\033[2;38;2;255;158;100m"
TND_TEAL="\033[2;38;2;42;195;222m"
TND_YELLOW="\033[2;38;2;224;175;104m"
TND_GRAY="\033[2;38;2;86;95;137m"

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e " Tokyo Night Dim - Pipe │"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo -e " ${TND_BLUE}trips${RESET} ${TND_GRAY}│${RESET} ${TND_CYAN}d/api/endpoints${RESET}"
echo -e " ${TND_PURPLE}main${RESET} ${TND_GRAY}│${RESET} ${TND_MAGENTA}feature-auth${RESET} ${TND_GRAY}│${RESET} ${TND_GREEN}+3${RESET} ${TND_YELLOW}~2${RESET} ${TND_GRAY}│${RESET} ${TND_GRAY}↑1${RESET}"
echo -e " ${TND_ORANGE}Opus${RESET} ${TND_GRAY}│${RESET} ${TND_TEAL}84%${RESET} ${TND_GRAY}│${RESET} ${TND_BLUE}verbose${RESET} ${TND_GRAY}│${RESET} ${TND_YELLOW}◔ 35m${RESET}"
echo -e " ${TND_GRAY}47m${RESET} ${TND_GRAY}│${RESET} ${TND_GRAY}resets 12m${RESET} ${TND_GRAY}│${RESET} ${TND_GRAY}125K/42K${RESET}"
echo ""
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e " Tokyo Night Dim - Dot •"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo -e " ${TND_BLUE}trips${RESET} ${TND_GRAY}•${RESET} ${TND_CYAN}d/api/endpoints${RESET}"
echo -e " ${TND_PURPLE}main${RESET} ${TND_GRAY}•${RESET} ${TND_MAGENTA}feature-auth${RESET} ${TND_GRAY}•${RESET} ${TND_GREEN}+3${RESET} ${TND_YELLOW}~2${RESET} ${TND_GRAY}•${RESET} ${TND_GRAY}↑1${RESET}"
echo -e " ${TND_ORANGE}Opus${RESET} ${TND_GRAY}•${RESET} ${TND_TEAL}84%${RESET} ${TND_GRAY}•${RESET} ${TND_BLUE}verbose${RESET} ${TND_GRAY}•${RESET} ${TND_YELLOW}◔ 35m${RESET}"
echo -e " ${TND_GRAY}47m${RESET} ${TND_GRAY}•${RESET} ${TND_GRAY}resets 12m${RESET} ${TND_GRAY}•${RESET} ${TND_GRAY}125K/42K${RESET}"
echo ""
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e " Tokyo Night Normal - Pipe │"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo -e " ${TN_BLUE}trips${RESET} ${TN_GRAY}│${RESET} ${TN_CYAN}d/api/endpoints${RESET}"
echo -e " ${TN_PURPLE}main${RESET} ${TN_GRAY}│${RESET} ${TN_MAGENTA}feature-auth${RESET} ${TN_GRAY}│${RESET} ${TN_GREEN}+3${RESET} ${TN_YELLOW}~2${RESET} ${TN_GRAY}│${RESET} ${TN_GRAY}↑1${RESET}"
echo -e " ${TN_ORANGE}Opus${RESET} ${TN_GRAY}│${RESET} ${TN_TEAL}84%${RESET} ${TN_GRAY}│${RESET} ${TN_BLUE}verbose${RESET} ${TN_GRAY}│${RESET} ${TN_YELLOW}◔ 35m${RESET}"
echo -e " ${TN_GRAY}47m${RESET} ${TN_GRAY}│${RESET} ${TN_GRAY}resets 12m${RESET} ${TN_GRAY}│${RESET} ${TN_GRAY}125K/42K${RESET}"
echo ""
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e " Tokyo Night Normal - Dot •"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo -e " ${TN_BLUE}trips${RESET} ${TN_GRAY}•${RESET} ${TN_CYAN}d/api/endpoints${RESET}"
echo -e " ${TN_PURPLE}main${RESET} ${TN_GRAY}•${RESET} ${TN_MAGENTA}feature-auth${RESET} ${TN_GRAY}•${RESET} ${TN_GREEN}+3${RESET} ${TN_YELLOW}~2${RESET} ${TN_GRAY}•${RESET} ${TN_GRAY}↑1${RESET}"
echo -e " ${TN_ORANGE}Opus${RESET} ${TN_GRAY}•${RESET} ${TN_TEAL}84%${RESET} ${TN_GRAY}•${RESET} ${TN_BLUE}verbose${RESET} ${TN_GRAY}•${RESET} ${TN_YELLOW}◔ 35m${RESET}"
echo -e " ${TN_GRAY}47m${RESET} ${TN_GRAY}•${RESET} ${TN_GRAY}resets 12m${RESET} ${TN_GRAY}•${RESET} ${TN_GRAY}125K/42K${RESET}"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
