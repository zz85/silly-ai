const AGENTS = {
  strategist: "Big picture, ROI, priorities. Bullet response, 1-2 lines. Direct.",
  engineer: "Technical feasibility, tradeoffs. Bullet response, 1-2 lines. Direct.",
  advocate: "Challenge assumptions, risks. Bullet response, 1-2 lines. Direct.",
  architect: "System design, scalability. Bullet response, 1-2 lines. Direct.",
  cheerleader: "Find positives, encourage! Friendly, 1-2 sentences.",
  joker: "One witty one-liner.",
  researcher: "Key facts others miss. Bullet response, 1-2 lines. Direct.",
  noter: "Summarize key points. Bullet response. Direct.",
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
