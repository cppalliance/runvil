# Web Plugin

This crate holds the web Pack and the Plugin that inserts it into a Host's Pack registry. The Pack gives a prompt a fetch tool and a search tool as one pair. The fetch tool is the security boundary between a model-chosen URL and the network: it revalidates every resolved address and every redirect, and it refuses non-public destinations unless configuration grants an exact exception. The search tool runs each search through the provider the Host supplies, so the search credential stays with that provider.
