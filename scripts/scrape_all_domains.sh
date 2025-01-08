#!/bin/sh

if [ "$#" -ne 1 ]
then
	echo >&2 "Usage: $0 OUTPUT_DIR"
	exit 1
fi

output="$1"

for domain in $(python3 spider --list-domains)
do
	if [ ! -e "$output/$domain" ]
	then
		echo "$domain"
		python3 spider "$domain" "$output"
	fi
done