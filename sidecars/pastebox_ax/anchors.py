"""Text anchors, in UTF-16 AX offsets. External text stays in process memory only."""
from dataclasses import dataclass
from hashlib import sha256


def units(text: str) -> int:
    return len(text.encode("utf-16-le")) // 2


def index_at(text: str, offset: int) -> int:
    if offset < 0:
        raise ValueError("选区位置无效")
    # Strict decoding rejects offsets inside a surrogate pair.
    try:
        data = text.encode("utf-16-le")
        if offset * 2 > len(data):
            raise ValueError("选区超出文字范围")
        return len(data[:offset * 2].decode("utf-16-le"))
    except UnicodeError as error:
        raise ValueError("选区位于不完整的 Unicode 字符内") from error


def digest(text: str) -> bytes:
    return sha256(text.encode("utf-8")).digest()


def replace(text: str, selection: tuple[int, int], inserted: str) -> str:
    start, length = selection
    return text[:index_at(text, start)] + inserted + text[index_at(text, start + length):]


@dataclass(frozen=True)
class Anchor:
    location: int
    length: int
    fingerprint: bytes
    left: str
    selected: str
    right: str
    at_start: bool
    at_end: bool

    @classmethod
    def capture(cls, text: str, selection: tuple[int, int]) -> "Anchor":
        start, length = selection
        if length < 0:
            raise ValueError("选区长度无效")
        a, b = index_at(text, start), index_at(text, start + length)
        if b - a > 8192:
            raise ValueError("选区过长，请缩小选区后记录位置")
        return cls(start, length, digest(text), text[max(0, a - 64):a], text[a:b],
                   text[b:b + 64], a < 64, b + 64 > len(text))

    def resolve(self, text: str) -> tuple[int, int]:
        if digest(text) == self.fingerprint:
            return self.location, self.length
        needle = self.left + self.selected + self.right
        if not needle:
            raise ValueError("原空输入框已有内容，请重新记录位置")
        matches = []
        offset = 0
        while (offset := text.find(needle, offset)) != -1:
            if ((not self.at_start or offset == 0) and
                    (not self.at_end or offset + len(needle) == len(text))):
                matches.append(offset + len(self.left))
            if len(matches) > 1:
                break
            offset += 1
        if len(matches) != 1:
            raise ValueError("原选区附近的文字已变化或存在多个匹配，请重新记录位置")
        return units(text[:matches[0]]), self.length


def after_edit(selection: tuple[int, int], edited: tuple[int, int], size: int) -> tuple[int, int]:
    """Rebase a saved selection across one verified insertion. Right affinity at the caret."""
    start, length = selection
    edit_start, removed = edited
    end, edit_end = start + length, edit_start + removed
    if selection == edited:
        return edit_start + size, 0
    if start >= edit_end:
        return start + size - removed, length
    if end <= edit_start:
        return selection
    raise ValueError("原选区与已替换内容重叠，请重新记录位置")
