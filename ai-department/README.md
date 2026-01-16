# AI Department

A multi-agent AI roundtable that gives you diverse perspectives on any topic.

## Agents

strategist, engineer, advocate, architect, cheerleader, joker, researcher, noter, teacher, editor, therapist

## Setup

Requires [Bun](https://bun.sh) and [LM Studio](https://lmstudio.ai) running on localhost:1234.

## Usage

### Web UI
```bash
bun proxy.ts
# Open http://localhost:3000
```

### CLI
```bash
bun team.ts "Your topic here"

# Or pipe input
echo "Should we use microservices?" | bun team.ts

# Different model
MODEL=llama-3.2-3b-instruct bun team.ts "Your question"
```
