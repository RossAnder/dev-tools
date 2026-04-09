#!/usr/bin/env fish
# Line 1: CWD@Branch (changes) | model | tokens (pct)
# Line 2: 5h dots pct @reset | 7d dots pct @reset
# Requires: jq

set -l input (cat)
if test -z "$input"
    printf "Claude"
    exit 0
end

# ANSI colors
set -l red    \e'[31m'
set -l green  \e'[32m'
set -l yellow \e'[33m'
set -l cyan   \e'[36m'
set -l white  \e'[37m'
set -l orange \e'[38;5;208m'
set -l dim    \e'[90m'
set -l rst    \e'[0m'

set -l dot_fill '●'
set -l dot_empty '○'

# ===== Helper functions =====

function format_tokens -a num
    if test "$num" -ge 1000000
        printf "%.1fm" (math "$num / 1000000")
    else if test "$num" -ge 1000
        printf "%.0fk" (math "$num / 1000")
    else
        printf "%s" "$num"
    end
end

function usage_color -a pct
    if test "$pct" -ge 90
        printf '\e[31m'
    else if test "$pct" -ge 70
        printf '\e[38;5;208m'
    else if test "$pct" -ge 50
        printf '\e[33m'
    else
        printf '\e[32m'
    end
end

function usage_dots -a pct
    set -l filled (math "ceil($pct / 20)")
    test "$filled" -gt 5; and set filled 5
    set -l colors '\e[32m' '\e[32m' '\e[33m' '\e[38;5;208m' '\e[31m'
    set -l rst \e'[0m'
    set -l dim \e'[90m'
    for i in (seq 1 5)
        if test "$i" -le "$filled"
            printf '%b●%b' "$colors[$i]" "$rst"
        else
            printf '%b○%b' "$dim" "$rst"
        end
    end
end

function format_epoch -a epoch style
    if test -z "$epoch" -o "$epoch" = "null" -o "$epoch" = "0"
        return 1
    end
    switch "$style"
        case time
            set -l result (date -d @"$epoch" '+%-l:%M%P' 2>/dev/null; or date -r "$epoch" '+%-l:%M%P' 2>/dev/null)
            # Strip :00 for on-the-hour times (7:00pm -> 7pm)
            echo (string replace ':00' '' "$result")
        case datetime
            set -l result (date -d @"$epoch" '+%-d/%-m, %-l:%M%P' 2>/dev/null; or date -r "$epoch" '+%-d/%-m, %-l:%M%P' 2>/dev/null)
            echo (string replace ':00' '' "$result")
        case '*'
            set -l result (date -d @"$epoch" '+%-d/%-m' 2>/dev/null; or date -r "$epoch" '+%-d/%-m' 2>/dev/null)
            echo "$result"
    end
end

# ===== Parse stdin with jq =====

set -l cwd          (echo "$input" | jq -r '.cwd // empty')
# Strip "Claude " prefix from model name (e.g. "Claude Opus 4.6" -> "Opus 4.6")
set -l model_name   (echo "$input" | jq -r '.model.display_name // empty' | string replace 'Claude ' '')
set -l size         (echo "$input" | jq -r '.context_window.context_window_size // 200000')
set -l pct_used     (echo "$input" | jq -r '.context_window.used_percentage // 0')
set -l exceeds_200k (echo "$input" | jq -r '.exceeds_200k_tokens // false')
set -l input_tok    (echo "$input" | jq -r '.context_window.current_usage.input_tokens // 0')
set -l cache_create (echo "$input" | jq -r '.context_window.current_usage.cache_creation_input_tokens // 0')
set -l cache_read   (echo "$input" | jq -r '.context_window.current_usage.cache_read_input_tokens // 0')
set -l has_limits   (echo "$input" | jq -r 'has("rate_limits")')
set -l five_pct     (echo "$input" | jq -r '.rate_limits.five_hour.used_percentage // 0')
set -l five_reset   (echo "$input" | jq -r '.rate_limits.five_hour.resets_at // 0')
set -l seven_pct    (echo "$input" | jq -r '.rate_limits.seven_day.used_percentage // 0')
set -l seven_reset  (echo "$input" | jq -r '.rate_limits.seven_day.resets_at // 0')

test "$size" -lt 200000 2>/dev/null; and set size 200000
set -l current (math "$input_tok + $cache_create + $cache_read")
set -l pct_int (math "floor($pct_used)")

# ===== Terminal width tiers =====
# Progressively hide elements as terminal narrows:
#   wide (>=100): everything, two lines
#   medium (>=70): hide git changes, two lines
#   narrow (>=50): hide model, reset times, two lines
#   compact (<50): single line: dir ctx% 5h:N% 7d:N%

set -l cols (tput cols 2>/dev/null; or echo 120)
set -l show_changes true
set -l show_model true
set -l show_resets true
set -l show_line2 true
set -l compact false

if test "$cols" -lt 100
    set show_changes false
end
if test "$cols" -lt 70
    set show_model false
    set show_resets false
end
if test "$cols" -lt 50
    set show_line2 false
    set compact true
end

# ===== Line 1 =====
set -l sep " $dim|$rst "
set -l line1 ""

if test -n "$cwd"
    set -l dir (basename "$cwd")
    set line1 "$cyan$dir$rst"

    set -l branch (git -C "$cwd" rev-parse --abbrev-ref HEAD 2>/dev/null)
    if test -n "$branch"
        set line1 "$line1$dim@$rst$green$branch$rst"

        if test "$show_changes" = "true"
            set -l added 0
            set -l deleted 0
            for stat_line in (git -C "$cwd" diff HEAD --numstat 2>/dev/null)
                set -l parts (string split \t "$stat_line")
                if string match -qr '^\d+$' "$parts[1]"
                    set added (math "$added + $parts[1]")
                end
                if string match -qr '^\d+$' "$parts[2]"
                    set deleted (math "$deleted + $parts[2]")
                end
            end
            if test (math "$added + $deleted") -gt 0
                set line1 "$line1 $dim($rst$green+$added$rst $red-$deleted$rst$dim)$rst"
            end
        end
    end
end

# Model
if test -n "$model_name" -a "$show_model" = "true"
    if test -n "$line1"
        set line1 "$line1$sep$dim$model_name$rst"
    else
        set line1 "$dim$model_name$rst"
    end
end

# Tokens
if test "$exceeds_200k" = "true"
    set -l tc $red
else
    set -l tc (usage_color "$pct_int")
end
set -l tok_color (test "$exceeds_200k" = "true"; and printf "$red"; or usage_color "$pct_int")
set -l tok_str (format_tokens "$current")"/"(format_tokens "$size")" $dim($rst$tok_color$pct_int%$rst$dim)$rst"

if test -n "$line1"
    set line1 "$line1$sep$tok_str"
else
    set line1 "$tok_str"
end

# ===== Line 2: Rate limits =====
set -l line2 ""

if test "$has_limits" = "true" -a "$show_line2" = "true"
    # 5-hour
    set -l p5 (math "floor($five_pct)")
    set -l c5 (usage_color "$p5")
    set -l d5 (usage_dots "$p5")
    set line2 "$white""5h$rst $d5 $c5$p5%$rst"
    if test "$show_resets" = "true"
        set -l r5 (format_epoch "$five_reset" time)
        if test -n "$r5"
            set line2 "$line2 $dim@$r5$rst"
        end
    end

    # 7-day
    set -l p7 (math "floor($seven_pct)")
    set -l c7 (usage_color "$p7")
    set -l d7 (usage_dots "$p7")
    set -l seg7 "$white""7d$rst $d7 $c7$p7%$rst"
    if test "$show_resets" = "true"
        set -l r7 (format_epoch "$seven_reset" datetime)
        if test -n "$r7"
            set seg7 "$seg7 $dim@$r7$rst"
        end
    end
    set line2 "$line2$sep$seg7"
end

# Output
if test "$compact" = "true"
    # Single-line compact: dir ctx% 5h:N% 7d:N%
    set -l cline ""
    if test -n "$cwd"
        set cline "$cyan"(basename "$cwd")"$rst"
    end
    set -l tok_color (test "$exceeds_200k" = "true"; and printf "$red"; or usage_color "$pct_int")
    set cline "$cline $tok_color$pct_int%$rst"
    if test "$has_limits" = "true"
        set -l p5 (math "floor($five_pct)")
        set -l p7 (math "floor($seven_pct)")
        set -l c5 (usage_color "$p5")
        set -l c7 (usage_color "$p7")
        set cline "$cline $dim|$rst $c5""5h:$p5%$rst $c7""7d:$p7%$rst"
    end
    printf '%b' "$cline"
else if test -n "$line2"
    printf '%b\n%b' "$line1" "$line2"
else
    printf '%b' "$line1"
end

exit 0
