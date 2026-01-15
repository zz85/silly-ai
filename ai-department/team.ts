const AGENTS = {
  strategist: "Strategic thinker. Big picture, priorities, business impact. Reply in 1-2 sentences max.",
  engineer: "Principal engineer. Technical feasibility and implementation. Reply in 1-2 sentences max.",
  advocate: "Devil's advocate. Challenge assumptions, find flaws. Reply in 1-2 sentences max.",
  architect: "Software architect. System design and scalability. Reply in 1-2 sentences max.",
  cheerleader: "Supportive cheerleader. Find positives, encourage. Reply in 1-2 sentences max.",
  joker: "Team joker. One witty one-liner only.",
  researcher: "Researcher. Key facts or context others miss. Reply in 1-2 sentences max.",
  noter: "Note-taker. Bullet the key points only.",
};

const MODEL = process.env.MODEL || "gemma-3-4b";
const LM_STUDIO_URL = process.env.LM_STUDIO_URL || "http://localhost:1234/v1/chat/completions";

async function askAgent(name: string, system: string, prompt: string, tokens = 100): Promise<string> {
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
        max_tokens: tokens,
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

  // Summarize all perspectives
  const allResponses = results.map(r => `${r.name}: ${r.response}`).join("\n");
  const summary = await askAgent(
    "summarizer",
    "Synthesize these team perspectives into a concise conclusion. 2-3 paragraphs max. Be direct.",
    `Topic: ${prompt}\n\nTeam input:\n${allResponses}`,
    500
  );
  console.log(`${"─".repeat(50)}\n📝 SYNTHESIS\n${summary}\n`);
}

main();
