import React from "react";

interface Props {
  children: React.ReactNode;
  resetKey: string;
}

interface State {
  error: Error | null;
}

export default class ErrorBoundary extends React.Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidUpdate(prevProps: Props) {
    if (prevProps.resetKey !== this.props.resetKey && this.state.error) {
      this.setState({ error: null });
    }
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("Panel render failed", error, info);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="error-card" role="alert">
          <h2>页面渲染失败</h2>
          <pre>{this.state.error.message}</pre>
          <div className="error-actions">
            <button type="button" onClick={() => this.setState({ error: null })}>
              重试当前页面
            </button>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
