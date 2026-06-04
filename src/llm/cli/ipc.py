#!/usr/bin/env python3
"""IPC bridge for communicating with ECC2 Rust orchestrator."""

import json
import sys
import os

# Import the core LLM types and get_provider
from llm.core.types import LLMInput, Message, Role, ToolCall, ToolDefinition
from llm.providers import get_provider

def handle_request(req_data: dict) -> None:
    try:
        input_data = req_data.get("input", {})
        
        # Convert dict to LLMInput
        messages = []
        for msg_dict in input_data.get("messages", []):
            role_str = msg_dict.get("role")
            role = Role(role_str) if role_str else Role.USER
            
            tool_calls = None
            if "tool_calls" in msg_dict:
                tool_calls = [
                    ToolCall(
                        id=tc["id"], 
                        name=tc["name"], 
                        arguments=tc.get("arguments", {})
                    )
                    for tc in msg_dict["tool_calls"]
                ]
                
            msg = Message(
                role=role,
                content=msg_dict.get("content", ""),
                name=msg_dict.get("name"),
                tool_call_id=msg_dict.get("tool_call_id"),
                tool_calls=tool_calls
            )
            messages.append(msg)
            
        tools = None
        if "tools" in input_data and input_data["tools"]:
            tools = [
                ToolDefinition(
                    name=td["name"],
                    description=td["description"],
                    parameters=td["parameters"],
                    strict=td.get("strict", True)
                )
                for td in input_data["tools"]
            ]
            
        llm_input = LLMInput(
            messages=messages,
            model=input_data.get("model", os.environ.get("LLM_MODEL")),
            temperature=input_data.get("temperature", 1.0),
            max_tokens=input_data.get("max_tokens"),
            tools=tools,
            stream=input_data.get("stream", False),
            metadata=input_data.get("metadata", {})
        )
        
        provider_name = os.environ.get("LLM_PROVIDER", "claude")
        provider = get_provider(provider_name)
        
        output = provider.generate(llm_input)
        
        response_msg = {
            "type": "response",
            "output": output.to_dict()
        }
        
        print(json.dumps(response_msg), flush=True)
        
    except Exception as e:
        error_msg = {
            "type": "error",
            "error": str(e)
        }
        print(json.dumps(error_msg), flush=True)

def main():
    for line in sys.stdin:
        if not line.strip():
            continue
            
        try:
            msg = json.loads(line)
            msg_type = msg.get("type")
            
            if msg_type == "request":
                handle_request(msg)
            else:
                error_msg = {"type": "error", "error": f"Unknown message type: {msg_type}"}
                print(json.dumps(error_msg), flush=True)
                
        except json.JSONDecodeError:
            error_msg = {"type": "error", "error": "Invalid JSON input"}
            print(json.dumps(error_msg), flush=True)

if __name__ == "__main__":
    main()
