#!/usr/bin/env python3

import argparse
import json
import os
import sys
from typing import Dict, List, Optional

import boto3


class CIFixer:
    def __init__(self, agent_id: str, agent_alias_id: str, region: str):
        """
        Initialize CIFixer with Bedrock agent configuration.
        
        Args:
            agent_id: The ID of the Bedrock agent to use
            agent_alias_id: The alias ID of the Bedrock agent
            region: AWS region where the agent is deployed
        """
        self.agent_id = agent_id
        self.agent_alias_id = agent_alias_id
        self.bedrock_agent_runtime = boto3.client(
            service_name="bedrock-agent-runtime",
            region_name=region
        )
    
    def fix_typos(self, typos_data: List[Dict]) -> None:
        """
        Process a list of typos and fix them using Bedrock agent.
        
        Args:
            typos_data: List of dictionaries containing typo information
        """
        fixed_count = 0
        
        for typo in typos_data:
            try:
                file_path = typo["path"]
                # Remove the './' prefix if present
                if file_path.startswith("./"):
                    file_path = file_path[2:]
                
                line_num = typo["line_num"]
                byte_offset = typo["byte_offset"]
                typo_word = typo["typo"]
                corrections = typo["corrections"]
                
                print(f"Fixing typo: '{typo_word}' -> {corrections} in {file_path} (line {line_num})")
                
                # Process the file and fix the typo
                if self._fix_typo_in_file(file_path, line_num, byte_offset, typo_word, corrections):
                    fixed_count += 1
                
            except KeyError as e:
                print(f"Error: Missing key in typo data: {e}")
            except Exception as e:
                print(f"Error processing typo in {typo.get('path', 'unknown file')}: {e}")
        
        print(f"\nFixed {fixed_count} typos out of {len(typos_data)} reported.")
    
    def _fix_typo_in_file(self, file_path: str, line_num: int, byte_offset: int, 
                         typo_word: str, corrections: List[str]) -> bool:
        """
        Fix a single typo in a file using the Bedrock agent.
        
        Args:
            file_path: Path to the file containing the typo
            line_num: Line number where the typo occurs
            byte_offset: Byte offset of the typo in the line
            typo_word: The word that is misspelled
            corrections: List of possible corrections
            
        Returns:
            bool: True if the typo was fixed, False otherwise
        """
        # Read the file content
        try:
            with open(file_path, 'r', encoding='utf-8') as f:
                file_content = f.read()
        except FileNotFoundError:
            print(f"File not found: {file_path}")
            return False
        except Exception as e:
            print(f"Error reading file {file_path}: {e}")
            return False
        
        # Send the content to Bedrock agent
        corrected_content = self._query_bedrock_agent(
            file_path=file_path,
            file_content=file_content,
            line_num=line_num,
            byte_offset=byte_offset,
            typo_word=typo_word,
            corrections=corrections
        )
        
        if not corrected_content:
            print(f"Failed to get corrected content from Bedrock agent for {file_path}")
            return False
            
        # Write the corrected content back to the file
        try:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(corrected_content)
            return True
        except Exception as e:
            print(f"Error writing corrected content to {file_path}: {e}")
            return False
    
    def _query_bedrock_agent(self, file_path: str, file_content: str, line_num: int, 
                           byte_offset: int, typo_word: str, corrections: List[str]) -> Optional[str]:
        """
        Query the Bedrock agent to fix the typo.
        
        Args:
            file_path: Path to the file
            file_content: Content of the file
            line_num: Line number where the typo occurs
            byte_offset: Byte offset of the typo in the line
            typo_word: The word that is misspelled
            corrections: List of possible corrections
            
        Returns:
            Optional[str]: Corrected file content or None if failed
        """
        try:
            # Construct the prompt for the agent
            prompt = self._construct_prompt(
                file_path=file_path,
                file_content=file_content,
                line_num=line_num,
                byte_offset=byte_offset,
                typo_word=typo_word,
                corrections=corrections
            )
            
            # Call the Bedrock agent
            response = self.bedrock_agent_runtime.invoke_agent(
                agentId=self.agent_id,
                agentAliasId=self.agent_alias_id,
                sessionId='ci-fixer-session',  # Using a fixed session ID for this purpose
                inputText=prompt
            )

            for event in response['completion']:
                event_type = list(event.keys())[0]
                data = event[event_type]

                if event_type == 'chunk':
                    response_text = data['bytes'].decode('utf-8')
            
            # Extract the corrected content from the agent's response
            corrected_content = self._extract_corrected_content(response_text)
            
            return corrected_content
            
        except Exception as e:
            print(f"Error querying Bedrock agent: {e}")
            return None
    
    def _construct_prompt(self, file_path: str, file_content: str, line_num: int,
                        byte_offset: int, typo_word: str, corrections: List[str]) -> str:
        """
        Construct a prompt for the Bedrock agent.
        
        Args:
            file_path: Path to the file
            file_content: Content of the file
            line_num: Line number where the typo occurs
            byte_offset: Byte offset of the typo in the line
            typo_word: The word that is misspelled
            corrections: List of possible corrections
            
        Returns:
            str: The prompt to send to the agent
        """
        # Calculate the context around the typo
        lines = file_content.split('\n')
        
        # Get the line with the typo
        typo_line = lines[line_num - 1] if line_num <= len(lines) else ""
        
        # Construct suggested correction (use the first correction)
        suggested_correction = corrections[0] if corrections else None
        
        # Build the prompt
        prompt = f"""
            I have a file with a typo that needs to be fixed. Here's the information:

            File: {file_path}
            Line number: {line_num}
            Typo: "{typo_word}"
            Suggested correction: "{suggested_correction}"

            The line with the typo looks like this:
            ```
            {typo_line}
            ```

            The typo is at byte offset {byte_offset} in this line.

            Here's the full content of the file:
            ```
            {file_content}
            ```

            Please fix the typo and return the ENTIRE corrected file content with nothing else.
            Please only fix those lines that are provided by this prompt and don't touch any other lines.
            Do note that every file should ends by one new line. You shouldn't remove that either.
            Do not include any explanations or markdown formatting in your response. Just return the corrected file content as plain text.
            """
        return prompt
    
    def _extract_corrected_content(self, response_text: str) -> Optional[str]:
        """
        Extract the corrected content from the agent's response.
        
        Args:
            response_text: The response text from the agent
            
        Returns:
            Optional[str]: The corrected content or None if not found
        """
        # The agent should be instructed to return just the corrected content
        # But we'll clean the response just in case, while preserving trailing newlines
        
        # Strip any markdown code block markers if present
        if response_text.startswith("```") and "```" in response_text[3:]:
            # Find the language identifier if present
            first_line_end = response_text.find('\n')
            if first_line_end > 3:
                # Skip the first line (```language)
                content_start = first_line_end + 1
            else:
                # Skip just the ``` marker
                content_start = 3
                
            # Find the closing code block
            content_end = response_text.rfind("```")
            
            # Extract the content between the markers
            if content_end > content_start:
                content = response_text[content_start:content_end]
                # Remove leading whitespace but preserve trailing newline
                content = content.lstrip()
                # Ensure the content ends with exactly one newline
                if not content.endswith('\n'):
                    content += '\n'
                return content
        
        # If no code block markers, use the full response but ensure trailing newline
        content = response_text.lstrip()
        # Ensure the content ends with exactly one newline
        if not content.endswith('\n'):
            content += '\n'
        return content


def parse_typo_json(json_data: str) -> List[Dict]:
    """
    Parse the JSON output from the typos command.
    
    Args:
        json_data: JSON string containing typo information
        
    Returns:
        List[Dict]: List of dictionaries with typo information
    """
    typos = []
    
    # First, try to parse the entire string as a JSON array
    try:
        data = json.loads(json_data)
        if isinstance(data, list):
            # If it's a JSON array, process each item
            for item in data:
                if isinstance(item, dict) and item.get('type') == 'typo':
                    typos.append(item)
            return typos
    except json.JSONDecodeError:
        # If parsing as an array fails, fall back to line-by-line parsing
        pass
    
    # Line-by-line parsing for JSONL format (each line is a separate JSON object)
    for line in json_data.strip().split('\n'):
        if not line.strip():
            continue
            
        try:
            typo_info = json.loads(line)
            if isinstance(typo_info, dict) and typo_info.get('type') == 'typo':
                typos.append(typo_info)
        except json.JSONDecodeError as e:
            print(f"Error parsing JSON: {e}")
    
    return typos


def main():
    parser = argparse.ArgumentParser(description='Fix CI issues using Amazon Bedrock agent')
    parser.add_argument('--agent-id', required=True, help='Bedrock agent ID')
    parser.add_argument('--agent-alias-id', required=True, help='Bedrock agent alias ID')
    parser.add_argument('--region', default='us-west-2', help='AWS region (default: us-west-2)')
    parser.add_argument('--typos-json', help='File containing typos JSON output (optional)')
    
    args = parser.parse_args()
    
    # Get the typos JSON data
    if args.typos_json:
        # Read from file
        try:
            with open(args.typos_json, 'r', encoding='utf-8') as f:
                json_data = f.read()
        except Exception as e:
            print(f"Error reading typos JSON file: {e}")
            return 1
    else:
        # Read from stdin
        json_data = sys.stdin.read()
    
    # Parse the JSON data
    typos = parse_typo_json(json_data)
    
    if not typos:
        print("No typos found to fix.")
        return 0
    
    print(f"Found {len(typos)} typos to fix.")
    
    # Initialize the CI fixer
    ci_fixer = CIFixer(
        agent_id=args.agent_id,
        agent_alias_id=args.agent_alias_id,
        region=args.region
    )
    
    # Fix the typos
    ci_fixer.fix_typos(typos)
    
    return 0


if __name__ == "__main__":
    sys.exit(main())
