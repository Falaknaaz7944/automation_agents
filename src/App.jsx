import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

function App() {
  const [isOpen, setIsOpen] = useState(false);

  const [messages, setMessages] = useState([
    { role: "assistant", content: "Hello 👋 How can I help you today?" },
  ]);

  const [input, setInput] = useState("");

  // Used when OpenClaw shows security warning / confirmation prompt
  const [pendingAction, setPendingAction] = useState(null);

  const sendMessage = async () => {
    if (!input.trim()) return;

    const userMessage = input.trim();
    setMessages((prev) => [...prev, { role: "user", content: userMessage }]);
    setInput("");

    try {
      let response;
      const lowerMsg = userMessage.toLowerCase();

      // ✅ 0) SETTINGS FIRST
      if (lowerMsg === "settings") {
        response = await invoke("show_settings");
      }

      // ✅ 1) SET KEY (Gemini only)
      else if (lowerMsg.startsWith("set key ")) {
        const key = userMessage.slice(8).trim();
        response = await invoke("set_llm_key", {
          llm_api_key: key,
          llm_provider: "gemini", // ✅ always gemini now
        });
      }

      // ✅ 2) Setup OpenClaw flow
      else if (lowerMsg === "setup openclaw") {
        response = await invoke("setup_openclaw");
      }

      // ✅ 3) Safe command routing to Rust command executor
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

      // ✅ 4) Everything else goes to LLM brain
      else {
        response = await invoke("llm_reply", { prompt: userMessage });
      }

      const text = String(response);

      setMessages((prev) => [...prev, { role: "assistant", content: text }]);

      // Detect OpenClaw security prompt and show UI buttons
      const lower = text.toLowerCase();
      if (
        lower.includes("inherently risky") ||
        lower.includes("continue?") ||
        lower.includes("security warning")
      ) {
        setPendingAction("openclaw_security_confirm");
      }
    } catch (e) {
      setMessages((prev) => [
        ...prev,
        {
          role: "assistant",
          content: `❌ Error: ${e?.toString?.() ?? "could not talk to backend."}`,
        },
      ]);
    }
  };

  const handleYesContinue = async () => {
    try {
      setMessages((prev) => [
        ...prev,
        { role: "user", content: "✅ Yes, continue." },
      ]);

      const res = await invoke("openclaw_finish_onboarding");

      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: String(res) },
      ]);

      setPendingAction(null);
    } catch (e) {
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: `❌ Failed to continue onboarding: ${String(e)}` },
      ]);
    }
  };

  const handleNoStop = () => {
    setMessages((prev) => [
      ...prev,
      { role: "user", content: "❌ No, stop." },
      {
        role: "assistant",
        content: "✅ Stopped. You can continue later from Settings.",
      },
    ]);
    setPendingAction(null);
  };

  const handleRunAudit = async () => {
    try {
      const res = await invoke("openclaw_security_audit");
      setMessages((prev) => [
        ...prev,
        {
          role: "assistant",
          content: "🔍 Security audit results:\n" + String(res),
        },
      ]);
    } catch (e) {
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: `❌ Failed to run security audit: ${String(e)}` },
      ]);
    }
  };

  return (
    <div className="app-container">
      {!isOpen && (
        <div className="center-message">
          <h1>Personaliz Desktop Assistant</h1>
          <p>Your automation assistant is ready.</p>
          <p style={{ opacity: 0.8, fontSize: 14 }}>
            Try: <b>setup openclaw</b>, <b>show logs</b>, <b>settings</b>,{" "}
            <b>set key {"<GEMINI_KEY>"}</b>, or <b>hello</b>
          </p>
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

            {pendingAction === "openclaw_security_confirm" && (
              <div style={{ marginTop: 10 }}>
                <div className="message assistant">
                  ⚠️ OpenClaw says this is powerful and risky. Do you want to continue?
                </div>

                <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
                  <button onClick={handleYesContinue}>✅ Yes</button>
                  <button onClick={handleNoStop}>❌ No</button>
                  <button onClick={handleRunAudit}>🔍 Run Audit</button>
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
