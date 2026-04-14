import type { RefObject } from "react";
import { useEffect } from "react";
import { MessageCard } from "./MessageCard";
import {
  Conversation,
  ConversationContent,
  ConversationScrollButton,
} from "./ai-elements/conversation";
import { Shimmer } from "./ai-elements/shimmer";
import { LoaderCircleIcon } from "lucide-react";
import type { FeedMessage } from "../lib/history";

interface ConversationFeedPanelProps {
  feedRef: RefObject<HTMLDivElement | null>;
  historyLoading: boolean;
  messages: FeedMessage[];
  highlightedFeedId: string | null;
  activeStageId: string | null;
  activeToolCallId: string | null;
  streaming?: boolean;
  onNavigateStage: (stageId: string) => void;
  onNavigateChildSession: (
    sessionId: string,
    context?: { stageId?: string | null; toolCallId?: string | null; label?: string | null },
  ) => void;
}

export function ConversationFeedPanel({
  feedRef,
  historyLoading,
  messages,
  highlightedFeedId,
  activeStageId,
  activeToolCallId,
  streaming = false,
  onNavigateStage,
  onNavigateChildSession,
}: ConversationFeedPanelProps) {
  // Sync feedRef to the Conversation scroll container
  useEffect(() => {
    if (!feedRef.current) return;
    feedRef.current.scrollTop = feedRef.current.scrollHeight;
  }, [feedRef, messages]);

  return (
    <Conversation className="h-full border-0 bg-transparent">
      <ConversationContent className="mx-auto w-full max-w-3xl px-6 py-4">
        {historyLoading ? (
          <div className="flex flex-1 items-center justify-center gap-2 text-muted-foreground py-12">
            <LoaderCircleIcon className="size-4 animate-spin" />
            <span className="text-sm">Loading history...</span>
          </div>
        ) : null}
        {!historyLoading && messages.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center text-muted-foreground/40">
            <span className="text-xs">Start a conversation</span>
          </div>
        ) : null}
        {messages.map((message) => (
          <MessageCard
            key={message.feedId}
            message={message}
            highlighted={highlightedFeedId === message.feedId}
            activeStageId={activeStageId}
            activeToolCallId={activeToolCallId}
            onNavigateStage={onNavigateStage}
            onNavigateChildSession={onNavigateChildSession}
          />
        ))}
        {streaming && messages.length > 0 && (
          <div className="px-1">
            <Shimmer as="span" className="text-sm" duration={1.5}>
              Thinking...
            </Shimmer>
          </div>
        )}
      </ConversationContent>
      <ConversationScrollButton />
    </Conversation>
  );
}
