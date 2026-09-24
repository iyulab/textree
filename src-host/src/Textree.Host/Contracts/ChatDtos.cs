namespace Textree.Host.Contracts;

// Replies always stream as server-sent events; there is no non-streaming mode to ask for.
public sealed record ChatRequestDto(List<ChatMessageDto> Messages, int? MaxTokens = null);
public sealed record ChatMessageDto(string Role, string Content);
