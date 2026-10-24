"""Shared secret-reference policy for CI selection and ephemeral runner admission."""
import re

import yaml


def expression_code(value):
    """Read expression boundaries without treating quoted braces as delimiters."""
    offset = 0
    while (start := value.find('${{', offset)) >= 0:
        index, quoted = start + 3, False
        while index < len(value):
            char = value[index]
            if char == "'":
                if quoted and value[index:index + 2] == "''":
                    index += 2
                    continue
                quoted = not quoted
            elif not quoted and char == '"':
                raise ValueError('unsupported expression quoting')
            elif not quoted and value[index:index + 2] == '}}':
                yield value[start + 3:index]
                offset = index + 2
                break
            index += 1
        else:
            raise ValueError('unterminated workflow expression')


def require_secret_free(source):
    """Refuse decoded secret keys and non-automatic secret expressions."""
    try:
        pending = [yaml.safe_load(source)]
    except yaml.YAMLError as error:
        raise ValueError('unverifiable workflow YAML') from error
    seen = set()
    while pending:
        value = pending.pop()
        if isinstance(value, (dict, list)):
            if id(value) in seen:
                continue
            seen.add(id(value))
        if isinstance(value, dict):
            if any(isinstance(key, str) and key.lower() == 'secrets' for key in value):
                raise ValueError('workflow declares or inherits secrets')
            pending.extend(value.values())
        elif isinstance(value, list):
            pending.extend(value)
        elif isinstance(value, str):
            for expression in expression_code(value):
                allowed = re.sub(r"\bsecrets\s*(?:\.\s*GITHUB_TOKEN\b|\[\s*'GITHUB_TOKEN'\s*\])", '', expression, flags=re.IGNORECASE)
                if re.search(r'\bsecrets\b', allowed, re.IGNORECASE):
                    raise ValueError('workflow references repository secrets')
