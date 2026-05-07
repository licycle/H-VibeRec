import { useEffect, useMemo, useState } from 'react';
import { Check, Eye, EyeOff, FileText, RefreshCw, X } from 'lucide-react';
import { SummaryTemplate } from '../appTypes';
import './SummaryTemplatePicker.css';

interface Props {
  open: boolean;
  title: string;
  description?: string;
  templates: SummaryTemplate[];
  loading: boolean;
  confirmLabel?: string;
  onRefresh: () => void | Promise<void>;
  onCancel: () => void;
  onConfirm: (template: SummaryTemplate) => void | Promise<void>;
}

export default function SummaryTemplatePicker({
  open,
  title,
  description,
  templates,
  loading,
  confirmLabel = '使用模板',
  onRefresh,
  onCancel,
  onConfirm,
}: Props) {
  const defaultTemplateId = useMemo(() => {
    return templates.find(template => template.id === 'builtin-meeting-minutes')?.id || templates[0]?.id || '';
  }, [templates]);
  const [selectedId, setSelectedId] = useState<string>('');
  const [previewId, setPreviewId] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setSelectedId(defaultTemplateId);
    setPreviewId(null);
  }, [defaultTemplateId, open]);

  if (!open) return null;

  const selectedTemplate = templates.find(template => template.id === selectedId) || null;

  return (
    <div className="summary-template-backdrop" onClick={onCancel}>
      <div className="summary-template-modal" onClick={event => event.stopPropagation()}>
        <div className="summary-template-header">
          <div>
            <div className="summary-template-title">{title}</div>
            {description && <div className="summary-template-description">{description}</div>}
          </div>
          <button className="icon-btn" onClick={onCancel} title="关闭">
            <X size={16} />
          </button>
        </div>

        <div className="summary-template-toolbar">
          <div className="summary-template-section-title">
            <FileText size={16} />
            总结模板
          </div>
          <button
            className="mini-btn"
            onClick={() => void onRefresh()}
            disabled={loading}
            title="刷新模板"
          >
            <RefreshCw size={16} className={loading ? 'spinning' : ''} />
          </button>
        </div>

        <div className="summary-template-body">
          {loading ? (
            <div className="summary-template-empty">
              <RefreshCw size={22} className="spinning" />
              <span>加载模板中...</span>
            </div>
          ) : templates.length === 0 ? (
            <div className="summary-template-empty">
              <span>暂无可用模板，请先在设置中添加总结模板。</span>
            </div>
          ) : (
            <div className="summary-template-list">
              {templates.map(template => {
                const selected = template.id === selectedId;
                const previewing = template.id === previewId;
                return (
                  <div key={template.id} className="summary-template-item-wrap">
                    <div
                      className={`summary-template-item ${selected ? 'selected' : ''}`}
                      onClick={() => setSelectedId(template.id)}
                    >
                      <div className="summary-template-info">
                        <div className="summary-template-name">
                          {template.name}
                          {template.is_builtin && <span className="summary-template-badge">内置</span>}
                        </div>
                        <div className="summary-template-copy">
                          {template.description || '无描述'}
                        </div>
                      </div>
                      <span
                        role="button"
                        tabIndex={0}
                        className={`summary-template-preview-btn ${previewing ? 'active' : ''}`}
                        title={previewing ? '收起模板内容' : '预览模板内容'}
                        onClick={event => {
                          event.stopPropagation();
                          setPreviewId(previewing ? null : template.id);
                        }}
                        onKeyDown={event => {
                          if (event.key !== 'Enter' && event.key !== ' ') return;
                          event.preventDefault();
                          event.stopPropagation();
                          setPreviewId(previewing ? null : template.id);
                        }}
                      >
                        {previewing ? <EyeOff size={15} /> : <Eye size={15} />}
                      </span>
                    </div>
                    {previewing && (
                      <pre className="summary-template-preview">{template.prompt}</pre>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>

        <div className="summary-template-footer">
          <button className="modal-secondary" onClick={onCancel}>取消</button>
          <button
            className="modal-primary"
            onClick={() => selectedTemplate && void onConfirm(selectedTemplate)}
            disabled={!selectedTemplate || loading}
          >
            <Check size={15} />
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
