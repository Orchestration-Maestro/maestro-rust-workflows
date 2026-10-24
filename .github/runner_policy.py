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
        root = yaml.compose(source, Loader=yaml.SafeLoader)
    except yaml.YAMLError as error:
        raise ValueError('unverifiable workflow YAML') from error
    if not isinstance(root, yaml.MappingNode):
        raise TypeError('workflow root is not a mapping')
    pending, seen = [root], set()
    while pending:
        node = pending.pop()
        if id(node) in seen:
            continue
        seen.add(id(node))
        if isinstance(node, yaml.MappingNode):
            for key, value in node.value:
                if isinstance(key, yaml.ScalarNode) and key.value.casefold() == 'secrets':
                    raise ValueError('workflow declares or inherits secrets')
                pending.extend((key, value))
        elif isinstance(node, yaml.SequenceNode):
            pending.extend(node.value)
        elif isinstance(node, yaml.ScalarNode):
            for expression in expression_code(node.value):
                allowed = re.sub(r"\bsecrets\s*(?:\.\s*GITHUB_TOKEN\b|\[\s*'GITHUB_TOKEN'\s*\])", '', expression, flags=re.IGNORECASE)
                if re.search(r'\bsecrets\b', allowed, re.IGNORECASE):
                    raise ValueError('workflow references repository secrets')
