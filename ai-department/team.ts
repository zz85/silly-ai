const AGENTS = {
  strategist: "You are a strategic thinker. Focus on big picture, priorities, business impact, and long-term implications. Be concise.",
  engineer: "You are a principal engineer. Focus on technical feasibility, code quality, and implementation details. Be concise.",
  advocate: "You are a devil's advocate. Challenge assumptions, find flaws, ask hard questions. Be concise.",
  architect: "You are a software architect. Focus on system design, patterns, scalability, and technical debt. Be concise.",
  cheerleader: "You are a supportive cheerleader. Find the positives, encourage, and motivate. Be concise.",
  joker: "You are the team joker. Add levity with a witty one-liner or observation. Keep it brief.",
  researcher: "You are a researcher. Provide relevant facts, references, or context that others might miss. Be concise.",
  noter: "You are a note-taker. Summarize the key points and action items from the discussion. Be concise.",
};

const MODEL = process.env.MODEL || "gemma-3-4b";
const LM_STUDIO_URL = process.env.LM_STUDIO_URL || "http://localhost:1234/v1/chat/completions";

async function askAgent(name: string, system: string, prompt: string): Promise<string> {
  try {
    const res = await fetch(LM_STUDIO_URL, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        model: MODEL,
        messages: [
          { role: "system", content: system },
          { role: "user", content: prompt },
        ],
        max_tokens: 300,
      }),
    });
    const data = await res.json();
    return data.choices?.[0]?.message?.content || "(no response)";
  } catch (e) {
    return `(error: ${e})`;
  }
}

async function main() {
  const prompt = process.argv.slice(2).join(" ") || (await Bun.stdin.text()).trim();
  if (!prompt) {
    console.log("Usage: bun team.ts <your prompt>\n   or: echo 'your prompt' | bun team.ts");
    process.exit(1);
  }

  console.log(`\n📋 Topic: ${prompt}\n${"─".repeat(50)}\n`);

  const results = await Promise.all(
    Object.entries(AGENTS).map(async ([name, system]) => ({
      name,
      response: await askAgent(name, system, prompt),
    }))
  );

  for (const { name, response } of results) {
    console.log(`🎭 ${name.toUpperCase()}\n${response}\n`);
  }
}

main();
