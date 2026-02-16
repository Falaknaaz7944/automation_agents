import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

function App() {
  const [isOpen, setIsOpen] = useState(false);
  const [messages, setMessages] = useState([
    { role: "assistant", content: "Hello 👋 How can I help you today?" },
  ]);
  const [input, setInput] = useState("");
  const [pendingAction, setPendingAction] = useState(null); // kept (not used much)
  const [pendingAgent, setPendingAgent] = useState(null);

  const sendMessage = async () => {
    if (!input.trim()) return;

    const userMessage = input.trim();
    setMessages((prev) => [...prev, { role: "user", content: userMessage }]);
    setInput("");

    try {
      let response = "";
      const lowerMsg = userMessage.trim().toLowerCase();

      // ----------------------------
      // ✅ SETTINGS
      // ----------------------------
      if (lowerMsg === "settings") {
        response = await invoke("show_settings");
      }

      // ----------------------------
      // ✅ CREATE AGENT (preview + approve)
      // ----------------------------
      else if (lowerMsg.startsWith("create agent:")) {
        const prompt = userMessage.replace(/^create agent:/i, "").trim();

        // Fallback parser (works even if LLM is down)
        const parseAgentFallback = (text) => {
          const toolsMatch = text.match(/tools\s*\[([^\]]*)\]/i);
          const tools = toolsMatch
            ? toolsMatch[1]
                .split(",")
                .map((t) => t.trim())
                .filter(Boolean)
            : [];

          const nameMatch = text.match(/name\s+(.+?)(?=\s+role|\s+goal|\s+tools|\s+schedule|\s+sandbox|$)/i);
          const name = nameMatch ? nameMatch[1].trim() : "Agent";

          const roleMatch = text.match(/role\s+(.+?)(?=\s+goal|\s+tools|\s+schedule|\s+sandbox|$)/i);
          const role = roleMatch ? roleMatch[1].trim() : "Assistant";

          const goalMatch = text.match(/goal\s+(.+?)(?=\s+tools|\s+schedule|\s+sandbox|$)/i);
          const goal = goalMatch ? goalMatch[1].trim() : "Help with tasks";

          const schedMatch = text.match(/schedule\s+([^\s]+)/i);
          const scheduleRaw = schedMatch ? schedMatch[1].trim().toLowerCase() : null;
          const schedule = scheduleRaw === "none" ? null : scheduleRaw;

          const sandboxMatch = text.match(/sandbox\s+([^\s]+)/i);
          const sandboxRaw = sandboxMatch ? sandboxMatch[1].trim().toLowerCase() : "off";
          const sandbox = sandboxRaw === "on" || sandboxRaw === "true";

          return {
            name,
            role,
            goal,
            tools,
            schedule,
            triggers: null,
            sandbox,
          };
        };

        try {
          // Try LLM first (nice-to-have)
          const llm = await invoke("llm_reply", {
            prompt:
              `Return ONLY valid JSON with keys: name, role, goal, tools (array), schedule (string or null), triggers (array or null), sandbox (true/false).\nUser request: ${prompt}`,
          });

          const raw = String(llm);
          const jsonStart = raw.indexOf("{");
          const jsonEnd = raw.lastIndexOf("}");
          const jsonText = raw.slice(jsonStart, jsonEnd + 1);
          const agentObj = JSON.parse(jsonText);

          setPendingAgent(agentObj);

          response =
            `🧩 Agent Preview Ready:\n\n` +
            `Name: ${agentObj.name}\n` +
            `Role: ${agentObj.role}\n` +
            `Goal: ${agentObj.goal}\n` +
            `Tools: ${(agentObj.tools || []).join(", ")}\n` +
            `Schedule: ${agentObj.schedule ?? "none"}\n` +
            `Sandbox: ${agentObj.sandbox ? "ON" : "OFF"}\n\n` +
            `✅ Approve or Cancel below.`;
        } catch (err) {
          // If LLM crashes, fallback instantly
          const agentObj = parseAgentFallback(prompt);
          setPendingAgent(agentObj);

          response =
            `🧩 Agent Preview Ready (fallback parser — LLM offline):\n\n` +
            `Name: ${agentObj.name}\n` +
            `Role: ${agentObj.role}\n` +
            `Goal: ${agentObj.goal}\n` +
            `Tools: ${(agentObj.tools || []).join(", ")}\n` +
            `Schedule: ${agentObj.schedule ?? "none"}\n` +
            `Sandbox: ${agentObj.sandbox ? "ON" : "OFF"}\n\n` +
            `✅ Approve or Cancel below.`;
        }
      }

      // ----------------------------
      // ✅ CLEAR KEY
      // ----------------------------
      else if (lowerMsg === "clear key" || lowerMsg === "remove key") {
        response = await invoke("clear_user_api_key");
      }

      // ----------------------------
      // ✅ SET KEY
      // ----------------------------
      else if (lowerMsg.startsWith("set key ")) {
        const rest = userMessage.slice(8).trim();
        const parts = rest.split(/\s+/);
        const provider = (parts[0] || "").trim();
        const key = parts.slice(1).join(" ").trim();

        if (!provider || !key) {
          response =
            "❌ Format:\nset key gemini <KEY>\nset key openai <KEY>\nset key claude <KEY>";
        } else {
          const normalized =
            provider.toLowerCase() === "claude"
              ? "anthropic"
              : provider.toLowerCase();

          response = await invoke("set_llm_key", {
            llmApiKey: key,
            llmProvider: normalized,
          });
        }
      }

      // ----------------------------
      // ✅ SETUP OPENCLAW
      // ----------------------------
      else if (lowerMsg === "setup openclaw") {
        response = await invoke("setup_openclaw");
      }

      // ----------------------------
      // ✅ LINKEDIN LOGIN
      // ----------------------------
      else if (lowerMsg === "linkedin login") {
        response = await invoke("linkedin_login");
      }

      // ----------------------------
      // ✅ LINKEDIN POST (manual)
      // ----------------------------
      else if (lowerMsg.startsWith("linkedin post:")) {
        const text = userMessage.replace(/^linkedin post:/i, "").trim();
        response = await invoke("linkedin_post", { text });
      }

      // ----------------------------
      // ✅ DEMO COMMANDS (EXACT)
      // ----------------------------
      
      // ----------------------------
      // ✅ LIST AGENTS
      // ----------------------------
      else if (lowerMsg === "list agents") {
        response = await invoke("list_agents");
      }

      // ----------------------------
      // ✅ PENDING APPROVALS
      // ----------------------------
      else if (lowerMsg === "pending approvals" || lowerMsg === "approvals") {
        response = await invoke("list_pending_approvals");
      }

      // ✅ APPROVE <id>
      else if (lowerMsg.startsWith("approve ")) {
        const id = userMessage.slice(8).trim();
        if (!id) response = "❌ Usage: approve <id>";
        else response = await invoke("approve_action", { id });
      }

      // ----------------------------
      // ✅ SAFE COMMANDS (no LLM)
      // ----------------------------
      else if (
        lowerMsg.startsWith("whoami") ||
        lowerMsg.startsWith("dir") ||
        lowerMsg.startsWith("echo") ||
        lowerMsg.startsWith("openclaw") ||
        lowerMsg.startsWith("node") ||
        lowerMsg.startsWith("npm") ||
        lowerMsg.startsWith("python") ||
        lowerMsg === "show logs" ||
        lowerMsg === "logs"
      ) {
        response = await invoke("send_message", { message: userMessage });
      }

      // ----------------------------
      // ✅ DEFAULT → LLM
      // ----------------------------
      // ✅ DEMO COMMANDS (never go to LLM)
else if (lowerMsg === "create demo agents") {
  response = await invoke("create_demo_agents");
}
else if (lowerMsg === "run demo1") {
  response = await invoke("run_demo1_once");
}
else if (lowerMsg === "run demo2") {
  response = await invoke("run_demo2_once");
}
else if (lowerMsg === "scheduler tick") {
  response = await invoke("scheduler_tick_now");
}

      else {
        response = await invoke("llm_reply", { prompt: userMessage });
      }

      const text = String(response);
      setMessages((prev) => [...prev, { role: "assistant", content: text }]);

      const lower = text.toLowerCase();
      if (lower.includes("inherently risky") || lower.includes("continue?")) {
        setPendingAction("openclaw_security_confirm");
      }
    } catch (e) {
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: `❌ Error: ${String(e)}` },
      ]);
    }
  };

  const handleApproveAgent = async () => {
    const n = String(pendingAgent?.name || "").trim();
    if (!n || n.toLowerCase() === "<name>" || /[<>]/.test(n)) {
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: "❌ Invalid agent name. Try again." },
      ]);
      return;
    }

    try {
      const res = await invoke("save_agent_config", {
        name: pendingAgent.name,
        role: pendingAgent.role,
        goal: pendingAgent.goal,
        toolsJson: JSON.stringify(pendingAgent.tools || []),
        schedule: pendingAgent.schedule ?? null,
        triggersJson: pendingAgent.triggers ? JSON.stringify(pendingAgent.triggers) : null,
        sandbox: !!pendingAgent.sandbox,
      });

      setMessages((prev) => [...prev, { role: "assistant", content: String(res) }]);
      setPendingAgent(null);
    } catch (e) {
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: `❌ Failed: ${String(e)}` },
      ]);
    }
  };

  const handleCancelAgent = () => {
    setPendingAgent(null);
    setMessages((prev) => [
      ...prev,
      { role: "assistant", content: "❌ Agent creation cancelled." },
    ]);
  };

  return (
    <div className="app-container">
      {!isOpen && (
        <div className="center-message">
          <h1>Personaliz Desktop Assistant</h1>
          <p>Your automation assistant is ready.</p>
        </div>
      )}

      {isOpen && (
        <div className="chat-panel">
          <div className="chat-header">
            <span>Assistant</span>
            <button onClick={() => setIsOpen(false)}>✖</button>
          </div>

          <div className="chat-body">
            {messages.map((msg, index) => (
              <div
                key={index}
                className={msg.role === "user" ? "message user" : "message assistant"}
              >
                {msg.content}
              </div>
            ))}

            {pendingAgent && (
              <div style={{ marginTop: 10 }}>
                <div className="message assistant">✅ Approve this agent configuration?</div>
                <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
                  <button onClick={handleApproveAgent}>✅ Approve</button>
                  <button onClick={handleCancelAgent}>❌ Cancel</button>
                </div>
              </div>
            )}
          </div>

          <div className="chat-input">
            <input
              type="text"
              placeholder="Type a message..."
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && sendMessage()}
            />
            <button onClick={sendMessage}>Send</button>
          </div>
        </div>
      )}

      <button className="floating-button" onClick={() => setIsOpen(true)}>
        💬
      </button>
    </div>
  );
}

export default App;
